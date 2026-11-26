#include "Render/PublishedResource.hpp"
#include "Render/ResourceCaptureSet.hpp"
#include <array>
#include <atomic>
#include <barrier>
#include <cstdlib>
#include <functional>
#include <iostream>
#include <string>
#include <thread>

namespace {
void Check(bool value,const char* message) {
    if(!value) {std::cerr<<message<<'\n';std::abort();}
}
// COM-style resource owner: engine, publication, and GPU consumers hold actual
// independent references. There is no test-side registry keeping objects alive.
struct Resource {
    static inline std::atomic<unsigned> live{},destroyed{},addRefs{};
    std::atomic<unsigned long> refs{1};
    const unsigned serial;
    std::function<void()> onDestroy;
    explicit Resource(unsigned id):serial(id) {++live;}
    unsigned long AddRef() {++addRefs;return ++refs;}
    unsigned long Release() {
        const auto left=--refs;
        if(!left)delete this;
        return left;
    }
private:
    ~Resource() {if(onDestroy)onDestroy();--live;++destroyed;}
};
using Pointer=Microsoft::WRL::ComPtr<Resource>;
Pointer Make(unsigned id) {Pointer p;p.Attach(new Resource(id));return p;}
using Publication=cvr::gpu::PublishedResource<Resource>;

void Rebuilds() {
    Publication output;
    struct Receipt {Pointer source;uint64_t fence{};};
    std::array<Receipt,4> gpu;
    for(unsigned graph=1;graph<=1000;++graph) {
        const uint64_t completed=graph>3 ? graph-3 : 0;
        for(auto& receipt:gpu)if(receipt.fence<=completed)receipt.source.Reset();
        auto engine=Make(graph);
        Check(output.Publish(engine.Get()),"new graph output was not published");
        const auto refs=Resource::addRefs.load();
        Check(!output.Publish(engine.Get()),"unchanged output reported replacement");
        Check(Resource::addRefs.load()==refs,"unchanged output incurred AddRef overhead");
        Check(output.Identity()==engine.Get(),"publication identity disagrees with owner");
        auto& receipt=gpu[graph%gpu.size()];
        Check(!receipt.source,"consumer recycled an uncompleted GPU receipt");
        receipt={output.Acquire(),graph};
        engine.Reset(); // graph rebuild destroys the game's owner immediately
        Check(receipt.source->serial==graph,"GPU lease did not keep the selected graph alive");
        Check(Resource::live.load()<=4,"old graph outputs accumulated after completion");
    }
    Check(Resource::destroyed.load()>=996,"1000 graph rebuilds retained a session history");
    output.Publish(nullptr);
    Check(output.Identity()==nullptr && !output.Acquire(),"empty publication retained a current output");
    Check(Resource::live.load()>0,"unpublishing released in-flight GPU sources");
    // Toggle off / no subsequent ring reuse: the completion poll retires receipts.
    for(auto& receipt:gpu)receipt.source.Reset();
    Check(Resource::live.load()==0 && Resource::destroyed.load()==1000,"completed consumers retained graph heaps");
}
void Parallel() {
    Publication output;
    std::barrier start(4);
    std::array<std::jthread,4> workers;
    for(unsigned writer=0;writer<2;++writer)workers[writer]=std::jthread([&,writer] {
        start.arrive_and_wait();
        for(unsigned i=1;i<=20000;++i) {
            auto engine=Make(writer*20000+i);
            output.Publish(engine.Get());
            if((i%19)==0)std::this_thread::yield();
        }
    });
    for(unsigned reader=2;reader<4;++reader)workers[reader]=std::jthread([&] {
        std::array<Pointer,4> pending;
        start.arrive_and_wait();
        for(unsigned i=0;i<100000;++i) {
            auto source=output.Acquire();
            if(source)Check(source->serial>0 && source->serial<=40000,"acquired resource was destroyed or torn");
            pending[i%pending.size()]=std::move(source);
            if((i%31)==0)std::this_thread::yield();
        }
    });
    for(auto& worker:workers)worker.join();
    Check(Resource::live.load()==1,"concurrent publication leaked retired resources");
    auto final=output.Acquire();
    Check(final.Get()==output.Identity(),"serialized publication and identity diverged");
    output.Publish(nullptr);
    Check(Resource::live.load()==1,"clearing publication revoked the consumer's lease");
    final.Reset();
    Check(Resource::live.load()==0 && Resource::destroyed.load()==40000,"parallel ownership did not drain");
}
void ReentrantRelease() {
    Publication output;
    bool released=false;
    auto old=Make(1);output.Publish(old.Get());
    old->onDestroy=[&] { auto current=output.Acquire();Check(current && current->serial==2,"old release ran before publication");released=true; };
    old.Reset();
    auto next=Make(2);output.Publish(next.Get());
    Check(released,"old output was retained after replacement");
    next.Reset();output.Publish(nullptr);
    Check(Resource::live.load()==0,"publication did not release its final owner");
}
using CaptureSet=cvr::gpu::ResourceCaptureSet<Resource,24>;
void CaptureRebuilds() {
    CaptureSet capture;
    struct Receipt {Pointer source;unsigned fence{};};std::array<Receipt,4> gpu;
    capture.SetEnabled(true);
    for(unsigned graph=1;graph<=1000;++graph) {
        const auto completed=graph>3?graph-3:0;
        for(auto& receipt:gpu)if(receipt.fence<=completed)receipt.source.Reset();
        std::array<Pointer,24> engine;
        for(unsigned i=0;i<engine.size();++i){engine[i]=Make((graph-1)*24+i+1);capture.Observe(engine[i].Get(),64,10,i);}
        auto rejected=Make(100000+graph);
        const auto refs=Resource::addRefs.load();
        capture.Observe(rejected.Get(),128,28,25);
        capture.Observe(engine[0].Get(),128,28,99);
        Check(refs==Resource::addRefs.load(),"duplicate/full capture set AddRef'd a source");
        rejected.Reset();
        auto snapshot=capture.Acquire();
        Check(snapshot.count==24 && snapshot.entries[0].state==128 && snapshot.entries[0].tag==0,"snapshot lost coherent metadata/first tag");
        auto& receipt=gpu[graph%gpu.size()];Check(!receipt.source,"uncompleted source lease was recycled");
        // Only this copied source enters the existing queue-fenced slot. Unused
        // snapshot inputs and a failed/unsubmitted recording release locally.
        receipt={std::move(snapshot.entries[0].resource),graph};
        capture.Reset();snapshot={};for(auto& source:engine)source.Reset();
        Check(Resource::live.load()<=3,"graph reset leaked unused source owners");
        Check(receipt.source->serial==(graph-1)*24+1,"in-flight GPU source died at publication reset");
    }
    capture.SetEnabled(false);
    auto disabled=Make(200000);const auto refs=Resource::addRefs.load();
    capture.Observe(disabled.Get(),64,10,1);disabled.Reset();
    Check(capture.Empty() && capture.Acquire().count==0 && Resource::addRefs.load()==refs,"disabled diagnostic retained a resource");
    // Signal failure must retain receipts; normal completion/disable polling can
    // drain them without waiting for a new ring submission.
    Check(Resource::live.load()>0,"publication disable revoked GPU leases");
    for(auto& receipt:gpu)receipt.source.Reset();
    Check(Resource::live.load()==0,"completed capture sources were not retired");
}
void CaptureParallel() {
    CaptureSet capture;capture.SetEnabled(true);std::barrier start(4);
    std::array<std::jthread,4> workers;
    for(unsigned writer=0;writer<2;++writer)workers[writer]=std::jthread([&,writer] {
        start.arrive_and_wait();
        for(unsigned i=1;i<=10000;++i){auto source=Make(writer*10000+i);capture.Observe(source.Get(),64,10,writer);capture.Update(source.Get(),128);}
    });
    workers[2]=std::jthread([&]{start.arrive_and_wait();for(unsigned i=0;i<20000;++i){auto s=capture.Acquire();Check(s.count<=24,"capture bound exceeded");
        for(size_t j=0;j<s.count;++j)Check(s.entries[j].resource && s.entries[j].resource->serial<=20000,"reader acquired a destroyed/hole entry");}});
    workers[3]=std::jthread([&]{start.arrive_and_wait();for(unsigned i=0;i<20000;++i){capture.Reset();if(i%7==0){capture.SetEnabled(false);capture.SetEnabled(true);}}});
    for(auto& worker:workers)worker.join();capture.SetEnabled(false);
    Check(Resource::live.load()==0 && Resource::destroyed.load()==20000,"concurrent capture/reset overwrote an owning reference");
}
void CaptureReentrant() {
    CaptureSet capture;capture.SetEnabled(true);bool released=false;
    auto source=Make(1);capture.Observe(source.Get(),64,10,0);
    source->onDestroy=[&]{Check(capture.Acquire().count==0,"reset released under lock/before empty publication");released=true;};
    source.Reset();capture.Reset();Check(released && Resource::live.load()==0,"reset did not release its old resource");
}
}
int main(int argc,char** argv) {
    if(argc!=2)return 2;
    const std::string name=argv[1];
    if(name=="rebuilds")Rebuilds();
    else if(name=="parallel")Parallel();
    else if(name=="reentrant_release")ReentrantRelease();
    else if(name=="capture_rebuilds")CaptureRebuilds();
    else if(name=="capture_parallel")CaptureParallel();
    else if(name=="capture_reentrant")CaptureReentrant();
    else return 2;
    std::cout<<"PASS "<<name<<'\n';
}
