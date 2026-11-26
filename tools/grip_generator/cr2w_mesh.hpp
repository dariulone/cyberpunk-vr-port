#pragma once
// Read cooked meshes directly. Only one mesh and its render buffer are kept in
// memory. Geometry decoding and all subsequent numerical work run in C++.
#define NOMINMAX
#include <windows.h>
#include <nlohmann/json.hpp>
#include <algorithm>
#include <array>
#include <bit>
#include <cmath>
#include <cstdint>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <limits>
#include <span>
#include <stdexcept>
#include <string>
#include <string_view>
#include <vector>

namespace gripgen {
using Json=nlohmann::json;
using Bytes=std::vector<uint8_t>;
inline void Require(bool ok,std::string_view why){if(!ok)throw std::runtime_error(std::string(why));}
template<class T> T Read(std::span<const uint8_t> data,size_t at){
    Require(at<=data.size()&&sizeof(T)<=data.size()-at,"binary read outside file");
    T value;std::memcpy(&value,data.data()+at,sizeof(value));return value;
}
inline Bytes Load(const std::filesystem::path& path){
    auto native=std::filesystem::absolute(path).lexically_normal().wstring();
    // Extracted depot paths regularly exceed MAX_PATH. An extended absolute
    // path works even when the command-line executable has no app manifest.
    if(!native.starts_with(L"\\\\?\\"))native=native.starts_with(L"\\\\")?L"\\\\?\\UNC\\"+native.substr(2):L"\\\\?\\"+native;
    std::ifstream input(std::filesystem::path(native),std::ios::binary|std::ios::ate);Require(bool(input),"cannot open mesh");
    const auto size=input.tellg();Require(size>0&&uint64_t(size)<512ull*1024*1024,"invalid or oversized mesh");
    Bytes result(static_cast<size_t>(size));input.seekg(0);
    Require(bool(input.read(reinterpret_cast<char*>(result.data()),size)),"incomplete mesh read");return result;
}
class Oodle {
    using Decode=int64_t(__cdecl*)(const void*,int64_t,void*,int64_t,int,int,int,void*,int64_t,void*,void*,void*,int64_t,int);
    HMODULE module{};Decode decode{};
public:
    explicit Oodle(const std::filesystem::path& library){
        module=LoadLibraryW(std::filesystem::absolute(library).c_str());Require(module!=nullptr,"cannot load game Oodle library");
        decode=reinterpret_cast<Decode>(GetProcAddress(module,"OodleLZ_Decompress"));
        if(!decode){FreeLibrary(module);module=nullptr;throw std::runtime_error("Oodle decompressor missing");}
    }
    Oodle(const Oodle&)=delete;Oodle& operator=(const Oodle&)=delete;
    ~Oodle(){if(module)FreeLibrary(module);}
    Bytes Unpack(std::span<const uint8_t> packed,uint32_t rawSize){
        Require(rawSize>0&&rawSize<512u*1024*1024,"invalid render buffer size");
        if(packed.size()>=8&&Read<uint32_t>(packed,0)==0x4b52414b){
            Require(Read<uint32_t>(packed,4)==rawSize,"KARK size disagrees with buffer table");
            Bytes raw(rawSize);
            const auto got=decode(packed.data()+8,int64_t(packed.size()-8),raw.data(),rawSize,1,0,0,nullptr,0,nullptr,nullptr,nullptr,0,3);
            Require(got==rawSize,"Oodle render buffer decode failed");return raw;
        }
        Require(packed.size()==rawSize,"unrecognized compressed buffer");return Bytes(packed.begin(),packed.end());
    }
};
struct Field {std::string name,type;size_t at{},end{};};
class Cr2w {
    Bytes data;
    std::array<std::array<uint32_t,3>,10> tables{};
    std::vector<std::string> names;
    std::string StringAt(size_t at) const {
        Require(at<data.size(),"name outside CR2W");size_t end=at;
        while(end<data.size()&&data[end])++end;Require(end<data.size(),"unterminated CR2W name");
        return std::string(reinterpret_cast<const char*>(data.data()+at),end-at);
    }
    std::string Name(uint16_t id)const{Require(id<names.size(),"CR2W name index out of range");return names[id];}
    std::vector<Field> Fields(size_t& at,size_t end)const {
        Require(end<=data.size()&&at<end&&data[at++]==0,"not a CR2W property object");std::vector<Field> out;
        while(at+2<=end){
            const auto name=Read<uint16_t>(data,at);if(!name){at+=2;return out;}
            Require(at+8<=end,"truncated property header");const auto size=Read<uint32_t>(data,at+4);
            Require(size>=4&&size<=end-at-4,"invalid property size");
            out.push_back({Name(name),Name(Read<uint16_t>(data,at+2)),at+8,at+4+size});at+=4+size;
        }
        throw std::runtime_error("unterminated property object");
    }
    Json Object(size_t& at,size_t end,unsigned depth)const {
        Require(depth<32,"CR2W nesting limit");Json result=Json::object();
        for(const auto& f:Fields(at,end)){
            size_t valueAt=f.at;
            try {auto value=Value(f.type,valueAt,f.end,depth+1);Require(valueAt==f.end,"unparsed property tail");result[f.name]=std::move(value);}
            catch(const std::exception&){result[f.name]=Json{{"unsupported",f.type},{"offset",f.at},{"bytes",f.end-f.at}};}
        }
        return result;
    }
    template<class T> Json Scalar(size_t& at,size_t end)const{
        Require(sizeof(T)<=end-at,"short scalar");const T value=Read<T>(data,at);at+=sizeof(T);return value;
    }
    Json Value(const std::string& type,size_t& at,size_t end,unsigned depth)const {
        Require(at<=end&&end<=data.size(),"invalid property span");
        if(type=="Float")return Scalar<float>(at,end);
        if(type=="Double")return Scalar<double>(at,end);
        if(type=="Uint8"||type=="Bool")return Scalar<uint8_t>(at,end);
        if(type=="Int8")return Scalar<int8_t>(at,end);
        if(type=="Uint16")return Scalar<uint16_t>(at,end);
        if(type=="Int16")return Scalar<int16_t>(at,end);
        if(type=="Uint32")return Scalar<uint32_t>(at,end);
        if(type=="Int32")return Scalar<int32_t>(at,end);
        if(type=="Uint64")return Scalar<uint64_t>(at,end);
        if(type=="Int64")return Scalar<int64_t>(at,end);
        if(type=="CName")return Name(Scalar<uint16_t>(at,end).get<uint16_t>());
        if(type.starts_with("handle:")||type.starts_with("whandle:"))return Json{{"chunk",Scalar<int32_t>(at,end).get<int32_t>()-1}};
        if(type=="DataBuffer"||type=="SerializationDeferredDataBuffer")return Json{{"buffer",(Scalar<uint32_t>(at,end).get<uint32_t>()&0xffff)-1}};
        if(type.starts_with("array:")||type.starts_with("static:")){
            auto inner=type.substr(type.find(':')+1);
            if(type.starts_with("static:"))inner=inner.substr(inner.find(',')+1);
            Require(at+4<=end,"truncated array length");
            const auto count=Read<uint32_t>(data,at);at+=4;Require(count<2000000&&count<=end-at,"invalid array length");
            Json values=Json::array();for(uint32_t i=0;i<count;++i)values.push_back(Value(inner,at,end,depth+1));return values;
        }
        if(at+3<=end&&data[at]==0)return Object(at,end,depth+1);
        if(end-at==2)return Name(Scalar<uint16_t>(at,end).get<uint16_t>());
        throw std::runtime_error("unsupported property type "+type);
    }
public:
    explicit Cr2w(const std::filesystem::path& path):data(Load(path)){
        Require(Read<uint32_t>(data,0)==0x57325243,"not CR2W");
        for(size_t i=0;i<tables.size();++i)for(size_t j=0;j<3;++j)tables[i][j]=Read<uint32_t>(data,40+12*i+4*j);
        Require(tables[1][1]<65536,"too many CR2W names");
        for(uint32_t i=0;i<tables[1][1];++i)names.push_back(StringAt(size_t(tables[0][0])+Read<uint32_t>(data,size_t(tables[1][0])+8*i)));
    }
    Json Chunk(std::string_view wanted)const {
        Require(tables[4][1]<100000,"too many CR2W chunks");
        for(uint32_t i=0;i<tables[4][1];++i){
            const size_t entry=size_t(tables[4][0])+24*i;
            if(Name(Read<uint16_t>(data,entry))!=wanted)continue;
            const size_t size=Read<uint32_t>(data,entry+8);size_t at=Read<uint32_t>(data,entry+12);
            return Object(at,at+size,0);
        }
        throw std::runtime_error("mesh has no "+std::string(wanted));
    }
    Bytes Buffer(uint32_t index,Oodle& oodle)const {
        Require(index<tables[5][1],"buffer index out of range");const size_t entry=size_t(tables[5][0])+24*index;
        const size_t at=Read<uint32_t>(data,entry+8),size=Read<uint32_t>(data,entry+12);
        Require(at<=data.size()&&size<=data.size()-at,"buffer outside CR2W");
        return oodle.Unpack(std::span<const uint8_t>(data).subspan(at,size),Read<uint32_t>(data,entry+16));
    }
};
struct Vec3 {float x{},y{},z{};};
struct Mesh {std::vector<Vec3> vertices;std::vector<std::array<uint32_t,3>> triangles;unsigned lod{},chunks{};};
inline unsigned PackingSize(const std::string& type){
    if(type=="PT_Float1"||type=="PT_Float16_2"||type=="PT_Dec4"||type=="PT_UDec4"||type=="PT_Color"||type=="PT_UByte4"||type=="PT_UByte4N"||type=="PT_Byte4N"||type=="PT_Short2"||type=="PT_Short2N"||type=="PT_UShort2N")return 4;
    if(type=="PT_Short4N"||type=="PT_Short4"||type=="PT_UShort4N"||type=="PT_Float16_4"||type=="PT_Float2")return 8;
    if(type=="PT_Float3")return 12;if(type=="PT_Float4")return 16;
    if(type=="PT_Invalid")return 0;
    throw std::runtime_error("unsupported vertex packing "+type);
}
inline float Half(uint16_t h){
    const int exp=(h>>10)&31;const float mant=float(h&1023);
    const float x=exp==0?std::ldexp(mant,-24):exp==31?std::numeric_limits<float>::infinity():std::ldexp(1+mant/1024,exp-15);
    return h&0x8000?-x:x;
}
inline Mesh DecodeMesh(const Cr2w& cooked,Oodle& oodle){
    const auto blob=cooked.Chunk("rendRenderMeshBlob");const auto& header=blob.at("header");
    const auto buffer=cooked.Buffer(blob.at("renderBuffer").at("buffer").get<uint32_t>(),oodle);
    Require(header.contains("renderChunkInfos"),"legacy packed render chunks require a separate decoder");
    const auto& chunks=header.at("renderChunkInfos");Require(chunks.is_array()&&!chunks.empty(),"no render chunks");
    Mesh mesh;unsigned lodMask=0;for(const auto& c:chunks)lodMask|=c.value("lodMask",1u);
    Require(lodMask!=0,"no mesh LOD");mesh.lod=lodMask&(~lodMask+1);
    const auto& scale=header.at("quantizationScale");const auto& offset=header.at("quantizationOffset");
    const std::array<float,3> scales{scale.value("X",0.f),scale.value("Y",0.f),scale.value("Z",0.f)};
    const std::array<float,3> offsets{offset.value("X",0.f),offset.value("Y",0.f),offset.value("Z",0.f)};
    for(const auto& c:chunks){
        if(!(c.value("lodMask",1u)&mesh.lod))continue;
        // RED omits default zero-valued fields. Some vehicle meshes retain
        // empty material chunks; they contribute neither vertices nor faces.
        const uint32_t count=c.value("numVertices",0u),indexCount=c.value("numIndices",0u);
        if(!count||!indexCount)continue;
        const auto& vertex=c.at("chunkVertices");const auto& layout=vertex.at("vertexLayout");
        unsigned stream=0,positionOffset=0;std::string type;std::array<unsigned,8> running{};
        for(const auto& element:layout.at("elements")){
            const auto packing=element.value("type",std::string("PT_Invalid"));
            if(packing=="PT_Invalid")break;
            const auto si=element.value("streamIndex",0u);Require(si<running.size(),"invalid vertex stream");
            if(element.value("usage",std::string{})=="PS_Position"&&element.value("usageIndex",0u)==0){stream=si;positionOffset=running[si];type=packing;break;}
            running[si]+=PackingSize(packing);
        }
        Require(!type.empty(),"no position vertex element");
        const auto& strides=layout.at("slotStrides");const auto& starts=vertex.at("byteOffsets");
        Require(stream<strides.size()&&stream<starts.size(),"position stream missing");
        const size_t stride=strides.at(stream).get<unsigned>(),start=starts.at(stream).get<unsigned>()+positionOffset;
        const uint32_t base=uint32_t(mesh.vertices.size());
        Require(count<16000000&&stride>=PackingSize(type),"invalid vertex stride/count");
        for(uint32_t i=0;i<count;++i){
            const size_t at=start+size_t(i)*stride;std::array<float,3> v{};
            for(size_t axis=0;axis<3;++axis){
                if(type=="PT_Short4N")v[axis]=std::max(-1.f,float(Read<int16_t>(buffer,at+axis*2))/32767.f)*scales[axis]+offsets[axis];
                else if(type=="PT_Float3"||type=="PT_Float4")v[axis]=Read<float>(buffer,at+axis*4);
                else if(type=="PT_Float16_4")v[axis]=Half(Read<uint16_t>(buffer,at+axis*2));
                else throw std::runtime_error("unsupported position encoding "+type);
                Require(std::isfinite(v[axis]),"non-finite mesh vertex");
            }
            mesh.vertices.push_back({v[0],v[1],v[2]});
        }
        const auto& indices=c.at("chunkIndices");const auto encoding=indices.value("pe",std::string("IBCT_IndexUShort"));
        const size_t width=encoding=="IBCT_IndexUShort"?2:encoding=="IBCT_IndexUInt"?4:0;
        Require(width!=0,"unsupported index format");
        const size_t indexStart=header.at("indexBufferOffset").get<uint32_t>()+indices.value("teOffset",0u);
        Require(indexCount%3==0,"non-triangle index buffer");
        for(uint32_t i=0;i<indexCount;i+=3){
            std::array<uint32_t,3> tri{};
            for(unsigned k=0;k<3;++k){const size_t at=indexStart+size_t(i+k)*width;const auto index=width==2?Read<uint16_t>(buffer,at):Read<uint32_t>(buffer,at);Require(index<count,"index outside mesh chunk");tri[k]=base+index;}
            if(tri[0]!=tri[1]&&tri[0]!=tri[2]&&tri[1]!=tri[2])mesh.triangles.push_back(tri);
        }
        ++mesh.chunks;
    }
    Require(!mesh.vertices.empty()&&!mesh.triangles.empty(),"empty decoded mesh");return mesh;
}
}
