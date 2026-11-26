// Offline native optics readback inspection. No game/process/GPU access.
#include <nlohmann/json.hpp>
#include <algorithm>
#include <cmath>
#include <cstdint>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <limits>
#include <vector>
using J=nlohmann::json;namespace fs=std::filesystem;
double Half(uint16_t v){const unsigned e=(v>>10)&31,m=v&1023;double x=e==31?(m?std::numeric_limits<double>::quiet_NaN():std::numeric_limits<double>::infinity()):e?std::ldexp(double(1024+m),int(e)-25):std::ldexp(double(m),-24);return v&32768?-x:x;}
void U16(std::ostream& s,uint16_t v){s.put(char(v));s.put(char(v>>8));}
void U32(std::ostream& s,uint32_t v){U16(s,uint16_t(v));U16(s,uint16_t(v>>16));}
uint8_t Color(double x,bool linear){if(!std::isfinite(x))return 0;x=std::clamp(x,0.,1.);if(linear)x=x<=.0031308?12.92*x:1.055*std::pow(x,1/2.4)-.055;return uint8_t(std::clamp(std::lround(x*255),0l,255l));}
int main(int argc,char** argv)try{
 if(argc==2&&std::string(argv[1])=="self-test"){
  if(Half(0x3c00)!=1||Half(0x3800)!=.5||Half(0xbc00)!=-1||Half(1)!=std::ldexp(1.,-24)||std::isfinite(Half(0x7c00))||Color(1,true)!=255)throw std::runtime_error("half/preview conversion failed");
  std::cout<<"PASS half floats, subnormal/sign/nonfinite and preview conversion\n";return 0;
 }
 if(argc!=2)throw std::runtime_error("atlas metadata.json");J meta;std::ifstream(argv[1])>>meta;
 const unsigned w=meta.at("width"),h=meta.at("height"),fmt=meta.at("format");const bool half=fmt==9||fmt==10;
 if(!w||!h||w>4096||h>4096||w%2||h%2||(!half&&fmt!=27&&fmt!=28&&fmt!=29))throw std::runtime_error("unsupported atlas layout");
 const size_t bpp=half?8:4;const fs::path raw=meta.at("raw").get<std::string>();std::vector<uint8_t> bytes(size_t(w)*h*bpp);
 std::ifstream file(raw,std::ios::binary);file.read(reinterpret_cast<char*>(bytes.data()),bytes.size());if(size_t(file.gcount())!=bytes.size()||file.peek()!=std::char_traits<char>::eof())throw std::runtime_error("raw size mismatch");
 auto channel=[&](size_t pixel,unsigned c){const auto i=pixel*bpp+c*(half?2:1);return half?Half(uint16_t(bytes[i])|(uint16_t(bytes[i+1])<<8)):double(bytes[i])/255;};
 const fs::path folder=raw.parent_path()/(raw.stem().string()+"-preview");fs::create_directories(folder);J result={{"scope","Raw native ink atlas; FP16 preview clamps HDR and converts linear RGB to sRGB; not the final optical shader."},{"tiles",J::array()}};
 const char* names[]{"camera-top-left","front-top-right","unused-bottom-left","rear-bottom-right"};
 for(unsigned tile=0;tile<4;++tile){const unsigned tw=w/2,th=h/2,ox=(tile%2)*tw,oy=(tile/2)*th,row=(tw*3+3)&~3u;
  const fs::path path=folder/(std::string(names[tile])+".bmp");std::ofstream out(path,std::ios::binary);out.write("BM",2);U32(out,54+row*th);U32(out,0);U32(out,54);U32(out,40);U32(out,tw);U32(out,th);U16(out,1);U16(out,24);U32(out,0);U32(out,row*th);U32(out,0);U32(out,0);U32(out,0);U32(out,0);
  uint64_t nonfinite=0,lit=0;double maximum=0,sum=0;std::vector<double> levels;levels.reserve(size_t(tw)*th);
  for(unsigned y=0;y<th;++y){for(unsigned x=0;x<tw;++x){const size_t at=size_t(oy+y)*w+ox+x;double value=0;for(unsigned c=0;c<3;++c){const double v=channel(at,c);if(!std::isfinite(v))++nonfinite;else value=std::max(value,v);}levels.push_back(value);sum+=value;maximum=std::max(maximum,value);lit+=value>.02;}
   for(unsigned x=0;x<tw;++x){const size_t at=size_t(oy+th-1-y)*w+ox+x;for(int c=2;c>=0;--c)out.put(char(Color(channel(at,unsigned(c)),half)));}for(unsigned p=tw*3;p<row;++p)out.put(0);
  }
  std::sort(levels.begin(),levels.end());result["tiles"].push_back({{"tile",names[tile]},{"preview",path.string()},{"pixels",levels.size()},{"litAbove002",lit},{"max",maximum},{"mean",sum/levels.size()},{"p99",levels[std::min(levels.size()-1,size_t(.99*levels.size()))]},{"nonfiniteChannels",nonfinite}});
 }
 std::ofstream(folder/"tiles.json")<<result.dump(2)<<'\n';std::cout<<result.dump(2)<<'\n';return 0;
}catch(const std::exception& e){std::cerr<<e.what()<<'\n';return 1;}
