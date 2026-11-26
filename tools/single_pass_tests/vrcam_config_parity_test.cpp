// vrcam.json's scanners from Rust (vrcam_config.rs through vrcam_config_abi.rs) against the reference C++ ones in
// Stereo/VrcamConfig.hpp, output for output, on deterministic synthetic texts and names. Built without CVR_RUST_VRIK so
// the header keeps the reference; the Rust staticlib supplies cvr_vrcam_*. The header has no tests of its own.
#include "Stereo/VrcamConfig.hpp"
#include <cstdio>
#include <random>
#include <stdexcept>
#include <string>
#include <vector>

extern "C" {
uint64_t cvr_vrcam_cname_hash(const char*);
bool cvr_vrcam_string_span(const char*,size_t,const char*,size_t*,size_t*);
intptr_t cvr_vrcam_string_array(const char*,size_t,const char*,size_t*,size_t);
bool cvr_vrcam_derive_camera(const char*,char*,size_t);
int cvr_vrcam_enable_value(const char*);
bool cvr_vrcam_parse_resolution(const char*,int*,int*);
}
static void Require(bool ok,const char* message){if(!ok)throw std::runtime_error(message);}
namespace {
std::mt19937 random(0xC0FFEE);
uint32_t Pick(uint32_t n){return std::uniform_int_distribution<uint32_t>(0,n-1)(random);}
std::string Piece(){
    static const char* pieces[]{"\"component\"","\"virtualCamera\"","\"components\""," : ",":","\"","[","]",",",
        "\"vrcam_2560x2560\"","\"vrcam_feed_1920x1080\""," ","\t","\n","\r","\v","x","_comment","\"\"","{","}","1","0",
        "\"_comment\": \"the \\\"component\\\": field\"","abc","\"a\"","\"b\"","vrcam_","2048x2048"};
    return pieces[Pick(sizeof(pieces)/sizeof(*pieces))];
}
std::string Text(){std::string text;const auto parts=Pick(40);for(uint32_t i=0;i<parts;++i)text+=Piece();return text;}
std::string Name(){
    static const char* names[]{"vrcam_2560x2560","vrcam_1920x1080","vrcam_","vrcam_x","vrcam_12x","vrcam_ 7x 9","vrcam_-4x5",
        "vrcam_3x4z","vrcam_0x5","vrcam_99999999999x2","other_640x480","_10x10","noscore","vrcam_a_b_640x360","",
        "vrcam_+8x+9","vrcam_8X9","vrcam_8x9 ","vrcam_9223372036854775807x2","vrcam_9223372036854775808x2",
        "vrcam_99999999999999999999x2","vrcam_4294967303x4294967298","vrcam_-9223372036854775809x3","vrcam_2x-4294967295"};
    std::string name=names[Pick(sizeof(names)/sizeof(*names))];
    if(Pick(4)==0)name+=std::string(Pick(160),'q');
    return name;
}
}
int main() try {
    unsigned spans{},arrays{},derived{},resolutions{};
    for(unsigned sample=0;sample<20000;++sample){
        const auto text=Text();
        for(const char* key:{"component","virtualCamera","components","_comment","missing"}){
            size_t p1=7,l1=7,p2=7,l2=7;
            const bool a=cvr::json_find_string_span(text,key,&p1,&l1),b=cvr_vrcam_string_span(text.data(),text.size(),key,&p2,&l2);
            Require(a==b && p1==p2 && l1==l2,"String span differs");spans+=a;
            std::vector<std::string> x{"kept"},y{"kept"};
            const bool c=cvr::json_find_string_array(text,key,&x);
            const auto count=cvr_vrcam_string_array(text.data(),text.size(),key,nullptr,0);
            if(count>=0){std::vector<size_t> pairs(2*size_t(count));
                if(count)cvr_vrcam_string_array(text.data(),text.size(),key,pairs.data(),size_t(count));
                for(size_t i=0;i<pairs.size();i+=2)y.emplace_back(text,pairs[i],pairs[i+1]);}
            Require(c==(count>=0) && x==y,"String array differs");arrays+=c;
        }
        const auto name=Name();
        Require(cvr::cname_hash(name.c_str())==cvr_vrcam_cname_hash(name.c_str()),"CName hash differs");
        for(const size_t size:{size_t(128),size_t(12),size_t(1)}){
            char a[160],b[160];std::memset(a,0x5A,sizeof(a));std::memset(b,0x5A,sizeof(b));
            const bool x=cvr::vrcam_derive_camera(name.c_str(),a,size),y=cvr_vrcam_derive_camera(name.c_str(),b,size);
            if(x!=y || std::memcmp(a,b,sizeof(a))!=0){size_t at=0;while(at<sizeof(a) && a[at]==b[at])++at;
                std::fprintf(stderr,"name '%s' size %zu: C++ %d Rust %d, first difference at %zu (%02X vs %02X)\n",name.c_str(),size,x,y,at,
                    at<sizeof(a)?uint8_t(a[at]):0,at<sizeof(b)?uint8_t(b[at]):0);}
            Require(x==y && std::memcmp(a,b,sizeof(a))==0,"Derived camera differs");derived+=x;
        }
        int w1=-3,h1=-3,w2=-3,h2=-3;
        const bool r1=cvr::vrcam_parse_resolution(name.c_str(),&w1,&h1),r2=cvr_vrcam_parse_resolution(name.c_str(),&w2,&h2);
        if(r1!=r2 || w1!=w2 || h1!=h2)std::fprintf(stderr,"name '%s': C++ %d %d %d, Rust %d %d %d\n",name.c_str(),r1,w1,h1,r2,w2,h2);
        Require(r1==r2 && w1==w2 && h1==h2,"Resolution differs");resolutions+=r1;
        Require(cvr::vrcam_enable_value(name.c_str())==cvr_vrcam_enable_value(name.c_str()) &&
            cvr::vrcam_enable_value(text.c_str())==cvr_vrcam_enable_value(text.c_str()),"Enable value differs");
    }
    Require(cvr::cname_hash(nullptr)==cvr_vrcam_cname_hash(nullptr) && cvr::vrcam_enable_value(nullptr)==cvr_vrcam_enable_value(nullptr),
        "Null handling differs");
    std::printf("PASS vrcam scanner parity: %u spans, %u arrays, %u derived cameras, %u resolutions found, all outputs equal\n",
        spans,arrays,derived,resolutions);
    return 0;
} catch(const std::exception& e){std::fprintf(stderr,"%s\n",e.what());return 1;}
