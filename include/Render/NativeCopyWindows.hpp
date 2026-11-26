#pragma once
#include <d3d12.h>
#include <algorithm>
#include <atomic>
#include <memory>
#include <mutex>
#include <vector>

namespace cvr::stereo::copy_windows {
// Numeric cache entries are not read/ownership grants. The caller pins originals
// and validates every new source occurrence independently on the same recording.
struct Span {
    uint64_t resource{},gpu{},backing{},offset{},bytes{},destination{},occurrence{};
};
#if CVR_RUST_VRIK
// The registry and windows: rust/crates/cvr-render/src/copy_windows.rs (tools/single_pass_tests/copy_windows_test runs
// its cases against them in its Rust variant). A Window here is a face over a Rust window (owning from Create, borrowed
// inside Each); a Token holds one strong reference to a span's entry, which lives in Rust with this layout.
struct Entry {Span span;std::atomic<bool> valid{true};};
static_assert(sizeof(Span)==56 && sizeof(Entry)==64 && sizeof(D3D12_RESOURCE_BARRIER)==32,"mirrored by copy_windows.rs");
namespace detail {
extern "C" {
// Read on every draw: the open windows, one load before any call into Rust.
extern std::atomic<uint32_t> cvr_cw_active_windows;
const void* cvr_cw_create(uint64_t,uint64_t);
void cvr_cw_window_drop(const void*);
uint64_t cvr_cw_window(const void*,uint32_t,uint64_t,uint64_t);
const Entry* cvr_cw_find(const void*,uint64_t,uint64_t,uint64_t,uint64_t);
void cvr_cw_publish(const void*,const Span*);
bool cvr_cw_entry(const Entry*,uint32_t);
void cvr_cw_each(bool(*)(void*,const void*),void*);
void cvr_cw_global(uint32_t,uint64_t,uint64_t);
void cvr_cw_barriers(uint32_t,const D3D12_RESOURCE_BARRIER*);
}
}
class Token {
public:
    Token()=default;
    explicit Token(const Entry* entry)noexcept:entry_(entry){}
    Token(const Token& other)noexcept:entry_(other.entry_){if(entry_)detail::cvr_cw_entry(entry_,0);}
    Token(Token&& other)noexcept:entry_(std::exchange(other.entry_,nullptr)){}
    Token& operator=(Token other)noexcept{std::swap(entry_,other.entry_);return *this;}
    ~Token(){if(entry_)detail::cvr_cw_entry(entry_,1);}
    explicit operator bool()const noexcept{return entry_!=nullptr;}
    const Entry* operator->()const noexcept{return entry_;}
    const Entry& operator*()const noexcept{return *entry_;}
    const Entry* get()const noexcept{return entry_;}
private:
    const Entry* entry_{};
};
struct Registry {std::atomic<uint32_t>& active;};
inline Registry State(){return {detail::cvr_cw_active_windows};}
class Window {
public:
    Window(const void* window,bool owned)noexcept:window_(window),owned_(owned){}
    ~Window(){if(owned_)detail::cvr_cw_window_drop(window_);}
    Window(const Window&)=delete;Window& operator=(const Window&)=delete;
    Token Find(uint64_t resource,uint64_t offset,uint64_t bytes,uint64_t occurrence){
        return Token(detail::cvr_cw_find(window_,resource,offset,bytes,occurrence));
    }
    void Publish(Span span)noexcept{detail::cvr_cw_publish(window_,&span);}
    void Write(uint64_t resource){detail::cvr_cw_window(window_,0,resource,0);}
    void Address(uint64_t address,uint64_t bytes){detail::cvr_cw_window(window_,1,address,bytes);}
    void Close(){detail::cvr_cw_window(window_,2,0,0);}
    uint64_t List()const{return detail::cvr_cw_window(window_,3,0,0);}
    uint64_t Generation()const{return detail::cvr_cw_window(window_,4,0,0);}
private:
    const void* window_;bool owned_;
};
inline std::shared_ptr<Window> Create(uint64_t list,uint64_t generation){
    const auto* window=detail::cvr_cw_create(list,generation);if(!window)return {};
    try{return std::make_shared<Window>(window,true);}catch(...){detail::cvr_cw_window_drop(window);detail::cvr_cw_global(4,0,0);return {};}
}
// An action's exception fails observation closed, as before.
template<class Action>inline void Each(Action action)noexcept{
    if(!detail::cvr_cw_active_windows.load(std::memory_order_relaxed))return;
    const auto call=[](void* context,const void* window)->bool{
        try{Window view(window,false);(*static_cast<Action*>(context))(view);return true;}catch(...){return false;}
    };
    detail::cvr_cw_each(call,&action);
}
inline bool Valid(const Token& token){return token && detail::cvr_cw_entry(token.get(),2);}
inline void Global(uint32_t which,uint64_t a,uint64_t b=0)noexcept{
    if(detail::cvr_cw_active_windows.load(std::memory_order_relaxed))detail::cvr_cw_global(which,a,b);
}
inline void Write(ID3D12Resource* resource)noexcept{Global(0,reinterpret_cast<uintptr_t>(resource));}
inline void Address(uint64_t address,uint64_t bytes)noexcept{Global(1,address,bytes);}
inline void Unknown(ID3D12GraphicsCommandList* list)noexcept{Global(2,reinterpret_cast<uintptr_t>(list));}
inline void Reset(ID3D12GraphicsCommandList* list)noexcept{Global(3,reinterpret_cast<uintptr_t>(list));}
inline void Unavailable()noexcept{detail::cvr_cw_global(4,0,0);}
inline void Barriers(UINT count,const D3D12_RESOURCE_BARRIER* barriers)noexcept{
    if(detail::cvr_cw_active_windows.load(std::memory_order_relaxed))detail::cvr_cw_barriers(count,barriers);
}
#else
struct Entry {Span span;std::atomic<bool> valid{true};};
using Token=std::shared_ptr<Entry>;
class Window;
struct Registry {std::mutex mutex;std::vector<std::weak_ptr<Window>> windows;std::atomic<bool> lost{};std::atomic<UINT> active{};};
// Read on every draw. An atomic pointer instead of a function-local static: no thread-safe-init
// guard on each call, so this inlines to one load once the registry exists.
inline std::atomic<Registry*> registryPointer{nullptr};
inline Registry& State(){
    Registry* state=registryPointer.load(std::memory_order_acquire);
    if(!state){auto* fresh=new Registry;
        if(registryPointer.compare_exchange_strong(state,fresh,std::memory_order_acq_rel))state=fresh;else delete fresh;}
    return *state;
}
class Window {
public:
    Window(uint64_t list,uint64_t generation):list_(list),generation_(generation){++State().active;}
    ~Window(){Close();}
    Token Find(uint64_t resource,uint64_t offset,uint64_t bytes,uint64_t occurrence){
        if(State().lost || !bytes || offset>UINT64_MAX-bytes)return {};
        std::lock_guard lock(mutex_);if(!open_)return {};
        for(const auto& entry:entries_){const auto& s=entry->span;
            if(entry->valid && s.resource==resource && s.occurrence<occurrence && offset>=s.offset && offset-s.offset<=s.bytes && bytes<=s.bytes-(offset-s.offset))return entry;
        }return {};
    }
    void Publish(Span span)noexcept{try{
        if(State().lost || !span.resource || !span.gpu || !span.backing || span.gpu>UINT64_MAX-span.backing || !span.bytes ||
            span.offset>span.backing || span.bytes>span.backing-span.offset || !span.occurrence || span.destination>UINT64_MAX-span.bytes)return;
        std::lock_guard lock(mutex_);if(!open_)return;
        std::erase_if(entries_,[](const auto& e){return !e->valid;});
        if(entries_.size()>=32768)return;
        auto entry=std::make_shared<Entry>();entry->span=span;entries_.push_back(std::move(entry));
    }catch(...){State().lost=true;}}
    void Write(uint64_t resource){std::lock_guard lock(mutex_);for(auto& e:entries_)if(!resource || e->span.resource==resource)e->valid=false;}
    void Address(uint64_t address,uint64_t bytes){std::lock_guard lock(mutex_);
        for(auto& e:entries_)if(!address || !bytes || address>UINT64_MAX-bytes ||
            (address<e->span.gpu+e->span.offset+e->span.bytes && e->span.gpu+e->span.offset<address+bytes))e->valid=false;
    }
    void Close(){std::lock_guard lock(mutex_);if(open_){open_=false;--State().active;}for(auto& e:entries_)e->valid=false;entries_.clear();}
    uint64_t List()const{return list_;}
    uint64_t Generation()const{return generation_;}
private:
    std::mutex mutex_;std::vector<Token> entries_;uint64_t list_{},generation_{};bool open_=true;
};
inline std::shared_ptr<Window> Create(uint64_t list,uint64_t generation){
    if(!list || !generation)return {};auto& registry=State();std::lock_guard lock(registry.mutex);
    std::erase_if(registry.windows,[](const auto& window){return window.expired();});
    if(registry.windows.size()>=128 || registry.lost)return {};
    auto window=std::make_shared<Window>(list,generation);registry.windows.push_back(window);return window;
}
template<class Action>inline void Each(Action action)noexcept{
    if(!State().active.load(std::memory_order_relaxed))return;
    try{auto& state=State();std::lock_guard lock(state.mutex);
        for(auto at=state.windows.begin();at!=state.windows.end();){if(auto window=at->lock()){action(*window);++at;}else at=state.windows.erase(at);}
    }catch(...){State().lost=true;}
}
inline bool Valid(const Token& token){return token && token->valid.load(std::memory_order_acquire) && !State().lost;}
inline void Write(ID3D12Resource* resource)noexcept{Each([&](Window& window){window.Write(reinterpret_cast<uintptr_t>(resource));});}
inline void Address(uint64_t address,uint64_t bytes)noexcept{Each([&](Window& window){window.Address(address,bytes);});}
inline void Unknown(ID3D12GraphicsCommandList* list)noexcept{Each([&](Window& window){if(window.List()==reinterpret_cast<uintptr_t>(list))window.Write(0);});}
inline void Reset(ID3D12GraphicsCommandList* list)noexcept{Each([&](Window& window){if(window.List()==reinterpret_cast<uintptr_t>(list))window.Close();});}
inline void Unavailable()noexcept{State().lost=true;}
inline void Barriers(UINT count,const D3D12_RESOURCE_BARRIER* barriers)noexcept{
    if(!State().active.load(std::memory_order_relaxed))return;
    if(count && !barriers){Unavailable();return;}
    for(UINT i=0;i<count;++i){const auto& barrier=barriers[i];
        if(barrier.Type==D3D12_RESOURCE_BARRIER_TYPE_TRANSITION)Write(barrier.Transition.pResource);
        else if(barrier.Type==D3D12_RESOURCE_BARRIER_TYPE_ALIASING)Each([](Window& window){window.Write(0);});
        else if(barrier.Type==D3D12_RESOURCE_BARRIER_TYPE_UAV)Write(barrier.UAV.pResource);
    }
}
#endif
inline thread_local UINT readOnlyDepth{};
struct ReadOnlyScope {ReadOnlyScope(){++readOnlyDepth;}~ReadOnlyScope(){--readOnlyDepth;}
    ReadOnlyScope(const ReadOnlyScope&)=delete;ReadOnlyScope& operator=(const ReadOnlyScope&)=delete;};
inline void UnknownDraw(ID3D12GraphicsCommandList* list)noexcept{if(!readOnlyDepth)Unknown(list);}
}
