#include "Framegen/IdlePrepare.hpp"
#include <chrono>
#include <thread>
#include <iostream>
#include <cstdlib>
using namespace cvr::framegen;
int64_t now(){return std::chrono::duration_cast<std::chrono::nanoseconds>(std::chrono::steady_clock::now().time_since_epoch()).count();}
void check(bool v,const char* m){if(!v){std::cerr<<m<<'\n';std::exit(1);}}
struct Work {unsigned n{};bool ready{};int sleep{};int64_t readyAfter{};};
bool prepare(void* c){auto& w=*static_cast<Work*>(c);++w.n;if(w.sleep)Sleep(w.sleep);return w.ready && now()>=w.readyAfter;}
int main(){
 HANDLE timer=CreateWaitableTimerExW(nullptr,nullptr,CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,TIMER_ALL_ACCESS);
 HANDLE event=CreateEventW(nullptr,FALSE,FALSE,nullptr);check(timer&&event,"handles");
 auto arm=[&](int ms){LARGE_INTEGER due{};due.QuadPart=-ms*10000LL;check(SetWaitableTimer(timer,&due,0,nullptr,nullptr,FALSE),"timer");return now()+ms*1000000LL;};
 Work w{0,true,0,now()+3000000};SetEvent(event);const auto end=arm(25);std::jthread producer([&]{Sleep(5);SetEvent(event);});
 auto r=WaitWithPreparation(timer,event,end,prepare,&w,now);producer.join();
 check(r.success && w.n==2 && r.queued,"stale event drained; new capture awakens second bounded preparation attempt");
 w={0,true,0};r=WaitWithPreparation(timer,event,arm(12),prepare,&w,now);
 check(r.success && r.queued && w.n==1,"already available work queues once then respects original timer");
 w={};r=WaitWithPreparation(timer,event,arm(1),prepare,&w,now);check(r.success && w.n==0,"no preparation near handoff deadline");
 w={0,true,12};r=WaitWithPreparation(timer,event,arm(4),prepare,&w,now);
 check(r.success && r.overrunMs>0 && w.n==1,"callback overrun observed without extra hold");
 CloseHandle(event);CloseHandle(timer);std::cout<<"PASS: capture-event opportunity, fixed deadline, two-attempt bound and overrun accounting\n";
}
