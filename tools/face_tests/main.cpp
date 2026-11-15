#include "Anim/FaceInput.hpp"
#include <cstdio>
#include <limits>
#include <vector>
#include <cstdlib>
using namespace cvr::face;
using Bytes=std::vector<uint8_t>;
void Check(bool pass,const char* what){if(!pass){std::fprintf(stderr,"FAIL %s\n",what);std::exit(1);}}
void Word(Bytes& b,uint32_t x){for(int shift=24;shift>=0;shift-=8)b.push_back(uint8_t(x>>shift));}
void String(Bytes& b,std::string_view s){b.insert(b.end(),s.begin(),s.end());b.push_back(0);while(b.size()%4)b.push_back(0);}
Bytes Message(Channel channel,float value){Bytes b;String(b,Addresses[channel]);String(b,",f");Word(b,std::bit_cast<uint32_t>(value));return b;}
Bytes Bundle(const std::vector<Bytes>& messages){Bytes b;String(b,"#bundle");Word(b,0);Word(b,1);for(const auto& m:messages){Word(b,uint32_t(m.size()));b.insert(b.end(),m.begin(),m.end());}return b;}
int main(){
    Input input;
    Check(Receive(Bundle({Message(SmileL,.6f),Message(Jaw,.8f)}),input,100),"valid lower-face bundle");
    Check(Receive(Message(BlinkL,1),input,400),"separate eye packet");
    Check(!input.Fresh(SmileL,400) && input.Fresh(BlinkL,400),"regions expire independently");
    Check(std::abs(input.Weight(SmileL,450)-.5f)<.001f,"smooth ownership fade");
    Check(input.Weight(SmileL,550)==0 && input.Weight(BlinkL,300)==0,"expired and reversed time hand off");
    const auto count=input.accepted;
    for(float value:{-1.f,1.01f,std::numeric_limits<float>::infinity(),std::numeric_limits<float>::quiet_NaN()}) {
        Check(!Receive(Message(Jaw,value),input,500),"invalid coefficient rejected");
        Check(input.accepted==count && input.channels[Jaw].at==100,"invalid value cannot refresh a channel");
    }
    auto broken=Bundle({Message(SmileR,.9f),Message(Jaw,.3f)});broken.resize(broken.size()-4);
    Check(!Receive(broken,input,500) && !input.channels[SmileR].present,"damaged bundles publish nothing");
    auto nested=Message(SmileR,.9f);for(int i=0;i<9;++i)nested=Bundle({nested});
    Check(!Receive(nested,input,500),"bounded OSC nesting");
    auto badSize=Bundle({Message(Jaw,.5f)});for(int i=16;i<20;++i)badSize[i]=255;
    Check(!Receive(badSize,input,500),"oversized bundle element");
    auto padded=Message(Jaw,.5f);padded[padded.size()-5]=1;
    Check(!Receive(padded,input,500),"type padding validated");
    Bytes unknown;String(unknown,"/unrelated");String(unknown,",b");Word(unknown,0);
    Check(Receive(Bundle({unknown,Message(LowerConfidence,.8f)}),input,500),"unrelated Steam Link message ignored");
    Check(input.Fresh(LowerConfidence,500),"confidence received");
    Input zero;Check(Receive(Message(Jaw,.2f),zero,0) && zero.Fresh(Jaw,0),"zero timestamp is valid");
    std::puts("PASS split-bundle freshness, fade, confidence, malformed packets and bounded parsing");
}
