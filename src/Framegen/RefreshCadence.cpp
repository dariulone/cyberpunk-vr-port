#include "Framegen/RefreshCadence.hpp"
#include "xrfg/steamvr_delivery.hpp"
#include <windows.h>
#include <atomic>
#include <chrono>
#include <condition_variable>
#include <mutex>
#include <thread>

namespace cvr::framegen::refresh {
namespace {
std::atomic<bool> targeted{false};
std::atomic<uint64_t> accepted{};
// One atomic publishes period+timestamp together; high32 = milliseconds since
// system boot, low32 = nanoseconds. Tick arithmetic handles the49-day wrap.
// Use a seqlock-free single packed sample so a consumer never spins on a worker.
std::atomic<uint64_t> sample{};
void Publish(int64_t ns){
    const uint64_t stamp=uint32_t(GetTickCount64());
    sample.store(Credible(ns)?(stamp<<32)|uint32_t(ns):0,std::memory_order_release);
}
class Monitor {
    std::mutex control,waitMutex;std::condition_variable wake;
    std::thread worker;std::atomic<bool> stopping{false};
    void Halt(){targeted=false;stopping=true;wake.notify_all();if(worker.joinable())worker.join();sample=0;accepted=0;}
    void Run() noexcept {
        try {
            xrfg::SteamVrDelivery reader(true);
            const auto start=std::chrono::steady_clock::now();
            std::unique_lock lock(waitMutex);
            while(!stopping.load()) {
                if(wake.wait_for(lock,std::chrono::seconds(1),[&]{return stopping.load();}))break;
                if(accepted.load()<90||std::chrono::steady_clock::now()-start<std::chrono::seconds(3))continue;
                lock.unlock();
                reader.mark_established();
                const auto period=reader.display_period();
                Publish(period?period->count():0);
                lock.lock();
            }
        } catch(...){sample=0;} // fallback to runtime period; never interrupt rendering
    }
public:
    ~Monitor(){Halt();}
    void Start(bool target){std::lock_guard lock(control);Halt();if(!target)return;stopping=false;targeted=true;
        try{worker=std::thread([this]{Run();});}catch(...){targeted=false;}}
    void Stop(){std::lock_guard lock(control);Halt();}
};
Monitor& Get(){static Monitor monitor;return monitor;}
}
Observation Read(){
    const uint64_t packed=sample.load(std::memory_order_acquire);
    if(!packed)return {};
    const auto now=GetTickCount64();
    const uint32_t age=uint32_t(now)-uint32_t(packed>>32);
    if(age>6000)return {};
    return {int64_t(uint32_t(packed)),now-age};
}
bool Targeted(){return targeted.load(std::memory_order_relaxed);}
int64_t Resolve(int64_t raw){return Select(Targeted(),raw,Read(),GetTickCount64());}
void Submitted(){if(Targeted())accepted.fetch_add(1,std::memory_order_relaxed);}
void Start(bool steamvr,bool packageEnabled){Get().Start(steamvr && packageEnabled);}
void Stop(){Get().Stop();}
}
