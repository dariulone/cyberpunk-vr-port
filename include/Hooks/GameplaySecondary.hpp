#pragma once
#include <atomic>

namespace cvr::input {
// The pad's B/Exit binding is routed separately. Only publish a gameplay hold
// if the controller using logical B has been up since a change of owner.
struct GameplaySecondary {
    std::atomic<int> armedSide{-1};
    void Reset() {armedSide.store(-1,std::memory_order_relaxed);}
    bool Update(bool eligible,bool down,int physicalSide) {
        if(!eligible || !down) {
            armedSide.store(eligible ? physicalSide : -1,std::memory_order_relaxed);
            return false;
        }
        if(armedSide.load(std::memory_order_relaxed)==physicalSide)return true;
        Reset();
        return false;
    }
};
}
