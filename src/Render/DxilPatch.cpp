#include "Render/DxilPatch.hpp"
#if !CVR_RUST_VRIK
// The C++ build's patches; the Rust build's are rust/crates/cvr-render/src/dxil_patch.rs (the header calls them).
#include <cstring>

namespace cvr::dxil_patch {
namespace {
constexpr uint32_t Shifts[64]{7,12,17,22,7,12,17,22,7,12,17,22,7,12,17,22,5,9,14,20,5,9,14,20,5,9,14,20,5,9,14,20,
                              4,11,16,23,4,11,16,23,4,11,16,23,4,11,16,23,6,10,15,21,6,10,15,21,6,10,15,21,6,10,15,21};
constexpr uint32_t Sines[64]{
    0xd76aa478,0xe8c7b756,0x242070db,0xc1bdceee,0xf57c0faf,0x4787c62a,0xa8304613,0xfd469501,
    0x698098d8,0x8b44f7af,0xffff5bb1,0x895cd7be,0x6b901122,0xfd987193,0xa679438e,0x49b40821,
    0xf61e2562,0xc040b340,0x265e5a51,0xe9b6c7aa,0xd62f105d,0x02441453,0xd8a1e681,0xe7d3fbc8,
    0x21e1cde6,0xc33707d6,0xf4d50d87,0x455a14ed,0xa9e3e905,0xfcefa3f8,0x676f02d9,0x8d2a4c8a,
    0xfffa3942,0x8771f681,0x6d9d6122,0xfde5380c,0xa4beea44,0x4bdecfa9,0xf6bb4b60,0xbebfbc70,
    0x289b7ec6,0xeaa127fa,0xd4ef3085,0x04881d05,0xd9d4d039,0xe6db99e5,0x1fa27cf8,0xc4ac5665,
    0xf4292244,0x432aff97,0xab9423a7,0xfc93a039,0x655b59c3,0x8f0ccc92,0xffeff47d,0x85845dd1,
    0x6fa87e4f,0xfe2ce6e0,0xa3014314,0x4e0811a1,0xf7537e82,0xbd3af235,0x2ad7d2bb,0xeb86d391};

uint32_t Rotl(uint32_t x,uint32_t c){return (x<<c)|(x>>(32-c));}
void Transform(uint32_t state[4],const uint8_t* block) {
    uint32_t x[16];std::memcpy(x,block,64);
    uint32_t a=state[0],b=state[1],c=state[2],d=state[3];
    for(uint32_t i=0;i<64;++i) {
        uint32_t f,g;
        if(i<16){f=(b&c)|(~b&d);g=i;}
        else if(i<32){f=(d&b)|(~d&c);g=(5*i+1)%16;}
        else if(i<48){f=b^c^d;g=(3*i+5)%16;}
        else{f=c^(b|~d);g=(7*i)%16;}
        f+=a+Sines[i]+x[g];a=d;d=c;c=b;b+=Rotl(f,Shifts[i]);
    }
    state[0]+=a;state[1]+=b;state[2]+=c;state[3]+=d;
}
uint32_t Bits(float value){uint32_t word;std::memcpy(&word,&value,4);return word;}
// The 36 bits of the placeholder's VBR6 encoding, in stream order.
uint64_t Pattern(float value) {
    uint64_t bits=0;uint32_t word=Bits(value);
    for(int chunk=0;chunk<6;++chunk) {
        const uint64_t piece=(word&31)|(chunk<5?32u:0u);word>>=5;
        bits|=piece<<(chunk*6);
    }
    return bits;
}
constexpr size_t PatternBits=36;
}

Hash ContainerHash(const uint8_t* container,size_t size) {
    Hash out{};
    if(!container || size<=20)return out;
    const uint8_t* data=container+20;const size_t length=size-20;
    uint32_t state[4]{0x67452301,0xEFCDAB89,0x98BADCFE,0x10325476};
    const size_t full=length&~size_t(63);
    for(size_t at=0;at<full;at+=64)Transform(state,data+at);
    const size_t count=length-full;
    const uint32_t bits=uint32_t(length*8), last=uint32_t(length*2)|1;
    uint8_t block[64]{};
    if(63-count<8) {
        std::memcpy(block,data+full,count);block[count]=0x80;Transform(state,block);
        std::memset(block,0,64);
    } else {
        std::memcpy(block+4,data+full,count);block[4+count]=0x80;
    }
    std::memcpy(block,&bits,4);std::memcpy(block+60,&last,4);
    Transform(state,block);
    std::memcpy(out.data(),state,16);
    return out;
}
bool Sign(std::vector<uint8_t>& container) {
    if(container.size()<=20 || std::memcmp(container.data(),"DXBC",4))return false;
    const auto hash=ContainerHash(container.data(),container.size());
    std::memcpy(container.data()+4,hash.data(),16);return true;
}
bool BitcodeRange(const uint8_t* c,size_t size,size_t& start,size_t& length) {
    if(!c || size<32 || std::memcmp(c,"DXBC",4))return false;
    uint32_t parts;std::memcpy(&parts,c+28,4);
    if(parts>64 || 32+size_t(parts)*4>size)return false;
    for(uint32_t i=0;i<parts;++i) {
        uint32_t offset;std::memcpy(&offset,c+32+i*4,4);
        if(size_t(offset)+8>size)return false;
        uint32_t partSize;std::memcpy(&partSize,c+offset+4,4);
        if(std::memcmp(c+offset,"DXIL",4))continue;
        const size_t program=offset+8;
        if(size_t(partSize)<24 || program+partSize>size || std::memcmp(c+program+8,"DXIL",4))return false;
        uint32_t bitcodeOffset,bitcodeSize;std::memcpy(&bitcodeOffset,c+program+16,4);std::memcpy(&bitcodeSize,c+program+20,4);
        start=program+8+bitcodeOffset;length=bitcodeSize;
        return start+length<=program+partSize;
    }
    return false;
}
bool PlaceholderValue(float value){const auto word=Bits(value);return word>=0x3F800000u && word<0x40000000u;}
std::vector<size_t> FindFloat(const uint8_t* container,size_t size,float placeholder) {
    std::vector<size_t> found;size_t start,length;
    if(!PlaceholderValue(placeholder) || !BitcodeRange(container,size,start,length) || length*8<PatternBits)return found;
    const uint64_t want=Pattern(placeholder), mask=(uint64_t(1)<<PatternBits)-1;
    for(size_t bit=start*8;bit+PatternBits<=(start+length)*8;++bit) {
        uint64_t window=0;
        const size_t byte=bit/8, shift=bit%8;
        for(size_t i=0;i<6 && byte+i<size;++i)window|=uint64_t(container[byte+i])<<(8*i);
        if(((window>>shift)&mask)==want)found.push_back(bit);
    }
    return found;
}
bool WriteFloat(std::vector<uint8_t>& container,size_t bit,float value) {
    if(!PlaceholderValue(value) || (bit+PatternBits+7)/8>container.size())return false;
    const uint64_t bits=Pattern(value);
    for(size_t i=0;i<PatternBits;++i) {
        const size_t at=bit+i;auto& byte=container[at/8];const uint8_t m=uint8_t(1u<<(at%8));
        byte=uint8_t(((bits>>i)&1)?(byte|m):(byte&~m));
    }
    return true;
}
}
#endif
