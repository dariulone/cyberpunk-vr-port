#pragma once
#include <windows.h>
#include <cstdint>
#include <algorithm>
namespace cvr::framegen {
struct IdlePrepareResult {bool success{},queued{};unsigned attempts{};double cpuMs{},overrunMs{};};
// One XR owner, same already-armed pacing timer and absolute deadline. At most
// two nonblocking preparation attempts; never restart/extend the pacing hold.
template<class Now> IdlePrepareResult WaitWithPreparation(HANDLE timer,HANDLE wake,int64_t deadline,
    bool (*prepare)(void*),void* context,Now now){
    IdlePrepareResult result;
    auto attempt=[&]{
        if(!prepare || result.queued || result.attempts>=2 || deadline-now()<2000000)return;
        const auto start=now();++result.attempts;result.queued=prepare(context);
        const auto end=now();result.cpuMs+=double(end-start)*1e-6;
        result.overrunMs=std::max(result.overrunMs,double(std::max<int64_t>(0,end-deadline))*1e-6);
    };
    // Drain the notification for the capture already selected before inspecting
    // latest state. A later publication still wakes the second attempt.
    if(wake)WaitForSingleObject(wake,0);
    attempt();
    for(;;){
        const auto left=deadline-now();
        if(left<=0){CancelWaitableTimer(timer);result.success=true;return result;}
        const bool listen=wake && prepare && !result.queued && result.attempts<2 && left>=2000000;
        HANDLE handles[]{timer,wake};
        const auto wait=WaitForMultipleObjects(listen?2:1,handles,FALSE,DWORD((left+999999)/1000000+3));
        if(wait==WAIT_OBJECT_0){result.success=true;return result;}
        if(listen && wait==WAIT_OBJECT_0+1){attempt();continue;}
        return result;
    }
}
}
