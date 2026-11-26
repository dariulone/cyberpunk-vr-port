// Select skinned weapon-part triangles and generate their rigid glow shell.
// All vertex decoding, transforms, normal welding and GLB generation run here.
#include <nlohmann/json.hpp>
#include <algorithm>
#include <array>
#include <bit>
#include <cmath>
#include <cstdint>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <map>
#include <set>
#include <stdexcept>
#include <string>
#include <vector>
using J=nlohmann::json;
using V=std::array<double,3>;
using T=std::array<uint32_t,3>;
using Bytes=std::vector<uint8_t>;
using Matrix=std::array<std::array<double,4>,4>;
static_assert(std::endian::native==std::endian::little);
void Require(bool yes,const std::string& why){if(!yes)throw std::runtime_error(why);}
J Load(const std::filesystem::path& file){std::ifstream in(file);Require(bool(in),"cannot read "+file.string());J j;in>>j;return j;}
const J& Root(const J& j){return j.contains("Data")?j.at("Data").at("RootChunk"):j;}
std::string Name(const J& j){return j.is_string()?j.get<std::string>():j.at("$value").get<std::string>();}
Bytes Decode(const std::string& text){
    const std::string alphabet="ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";Bytes out;out.reserve(text.size()*3/4);
    uint32_t acc=0;int bits=0;
    for(char c:text){if(c=='=')break;if(c=='\n'||c=='\r'||c==' ')continue;const auto n=alphabet.find(c);Require(n!=std::string::npos,"invalid base64");
        acc=(acc<<6)|uint32_t(n);bits+=6;if(bits>=8){bits-=8;out.push_back(uint8_t(acc>>bits));}}
    return out;
}
std::string Encode(const Bytes& bytes){
    const char* a="ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";std::string out;
    for(size_t i=0;i<bytes.size();i+=3){const size_t n=std::min(size_t(3),bytes.size()-i);uint32_t v=uint32_t(bytes[i])<<16;
        if(n>1)v|=uint32_t(bytes[i+1])<<8;if(n>2)v|=bytes[i+2];out+=a[v>>18];out+=a[(v>>12)&63];out+=n>1?a[(v>>6)&63]:'=';out+=n>2?a[v&63]:'=';}
    return out;
}
template<class U> U Read(const Bytes& b,size_t at){Require(at<=b.size()&&sizeof(U)<=b.size()-at,"vertex/index outside render buffer");U v;std::memcpy(&v,b.data()+at,sizeof(v));return v;}
template<class U> void Add(Bytes& b,U v){const auto at=b.size();b.resize(at+sizeof(v));std::memcpy(b.data()+at,&v,sizeof(v));}
V Plus(V a,V b){for(int k=0;k<3;++k)a[k]+=b[k];return a;}
V Minus(V a,V b){for(int k=0;k<3;++k)a[k]-=b[k];return a;}
V Times(V a,double s){for(auto& v:a)v*=s;return a;}
double Dot(V a,V b){return a[0]*b[0]+a[1]*b[1]+a[2]*b[2];}
V Cross(V a,V b){return {a[1]*b[2]-a[2]*b[1],a[2]*b[0]-a[0]*b[2],a[0]*b[1]-a[1]*b[0]};}
V Unit(V a){const double n=std::sqrt(Dot(a,a));Require(n>1e-12&&std::isfinite(n),"invalid normal");return Times(a,1/n);}
V Vector(const J& j){return {j.at("X").get<double>(),j.at("Y").get<double>(),j.at("Z").get<double>()};}
V Transform(V p,const Matrix& m){V out{};for(int k=0;k<3;++k)out[k]=m[3][k]+p[0]*m[0][k]+p[1]*m[1][k]+p[2]*m[2][k];return out;}
Matrix NormalMatrix(const Matrix& m){
    Matrix inv{};const double a=m[0][0],b=m[0][1],c=m[0][2],d=m[1][0],e=m[1][1],f=m[1][2],g=m[2][0],h=m[2][1],i=m[2][2];
    const double determinant=a*(e*i-f*h)-b*(d*i-f*g)+c*(d*h-e*g);Require(std::abs(determinant)>1e-12,"singular bone frame");
    inv[0]={e*i-f*h,c*h-b*i,b*f-c*e,0};inv[1]={f*g-d*i,a*i-c*g,c*d-a*f,0};inv[2]={d*h-e*g,b*g-a*h,a*e-b*d,0};
    for(int r=0;r<3;++r)for(int k=0;k<3;++k)inv[r][k]/=determinant;return inv;
}
V Normal(V n,const Matrix& inverse){V out{};for(int k=0;k<3;++k)for(int r=0;r<3;++r)out[k]+=inverse[k][r]*n[r];return Unit(out);}
size_t Size(const std::string& type){
    static const std::map<std::string,size_t> sizes{{"PT_Float1",4},{"PT_Float2",8},{"PT_Float3",12},{"PT_Float4",16},{"PT_Float16_2",4},{"PT_Float16_4",8},
        {"PT_UShort1",2},{"PT_UShort2",4},{"PT_UShort4",8},{"PT_UShort4N",8},{"PT_Short1",2},{"PT_Short2",4},{"PT_Short4",8},{"PT_Short4N",8},
        {"PT_UInt1",4},{"PT_UInt2",8},{"PT_UInt3",12},{"PT_UInt4",16},{"PT_Int1",4},{"PT_Int2",8},{"PT_Int3",12},{"PT_Int4",16},
        {"PT_Color",4},{"PT_UByte1",1},{"PT_UByte1F",1},{"PT_UByte4",4},{"PT_UByte4N",4},{"PT_Byte4N",4},{"PT_Dec4",4}};
    const auto it=sizes.find(type);Require(it!=sizes.end(),"unsupported vertex packing "+type);return it->second;
}
struct Element{std::string usage,type;int index{},stream{};size_t offset{},stride{},start{};};
std::vector<Element> Layout(const J& chunk){
    std::vector<Element> out;std::map<int,size_t> strides;const auto& vertices=chunk.at("chunkVertices");
    for(const auto& e:vertices.at("vertexLayout").at("elements").at("Elements")){
        const auto type=e.at("type").get<std::string>();if(type=="PT_Invalid"||e.value("streamType",std::string("ST_PerVertex"))!="ST_PerVertex")continue;const int stream=e.at("streamIndex").get<int>();
        Element x{e.at("usage"),type,e.value("usageIndex",0),stream,strides[stream],0,vertices.at("byteOffsets").at("Elements").at(stream).get<size_t>()};
        strides[stream]+=Size(type);out.push_back(x);
    }
    for(auto& x:out)x.stride=strides.at(x.stream);return out;
}
const Element& Find(const std::vector<Element>& elements,const char* usage,int index=0){
    for(const auto& e:elements)if(e.usage==usage&&e.index==index)return e;throw std::runtime_error(std::string("missing ")+usage);
}
struct Geometry{std::vector<V> positions,normals;std::vector<T> triangles;size_t sourceTriangles{};};
Geometry Generate(const std::vector<J>& meshes,const std::string& frameName,const std::set<std::string>& selected,double inflate){
    Matrix frame{};bool found=false;
    for(const auto& document:meshes){const auto& root=Root(document);if(!root.contains("boneNames"))continue;const auto& names=root.at("boneNames");
        for(size_t i=0;i<names.size();++i)if(Name(names[i])==frameName){const auto& m=root.at("boneRigMatrices").at(i);const char* axes[]{"X","Y","Z","W"};
            for(int r=0;r<4;++r)for(int k=0;k<4;++k)frame[r][k]=m.at(axes[r]).at(axes[k]).get<double>();found=true;break;}if(found)break;}
    if(!found)return {};const auto normalMatrix=NormalMatrix(frame);Geometry result;
    for(const auto& document:meshes){const auto& root=Root(document);if(!root.contains("boneNames"))continue;std::set<uint32_t> bones;const auto& names=root.at("boneNames");
        for(size_t i=0;i<names.size();++i)if(selected.contains(Name(names[i])))bones.insert(uint32_t(i));if(bones.empty())continue;
        const auto& blob=root.at("renderResourceBlob").at("Data");const auto& header=blob.at("header");const auto buffer=Decode(blob.at("renderBuffer").at("Bytes"));
        const V scale=Vector(header.at("quantizationScale")),offset=Vector(header.at("quantizationOffset"));
        for(const auto& chunk:header.at("renderChunkInfos")){
            if(!(chunk.value("lodMask",1u)&1))continue;const uint32_t nv=chunk.at("numVertices"),ni=chunk.at("numIndices");Require(ni%3==0,"non-triangle chunk");result.sourceTriangles+=ni/3;
            const auto elements=Layout(chunk);const auto& pe=Find(elements,"PS_Position");const auto& ne=Find(elements,"PS_Normal");
            Require(pe.type=="PT_Short4N"&&ne.type=="PT_Dec4","unsupported position/normal layout");std::vector<V> pos(nv),norm(nv);std::vector<double> weights(nv);
            for(uint32_t v=0;v<nv;++v){for(int k=0;k<3;++k)pos[v][k]=Read<int16_t>(buffer,pe.start+v*pe.stride+pe.offset+2*k)/32767.0*scale[k]+offset[k];
                const auto packed=Read<uint32_t>(buffer,ne.start+v*ne.stride+ne.offset);for(int k=0;k<3;++k)norm[v][k]=double((packed>>(10*k))&1023)/1023*2-1;
                for(const auto& e:elements)if(e.usage=="PS_SkinIndices"){
                    const auto& we=Find(elements,"PS_SkinWeights",e.index);Require(e.type=="PT_UByte4"&&we.type=="PT_UByte4N","unsupported skin layout");
                    for(int k=0;k<4;++k){const auto bone=Read<uint8_t>(buffer,e.start+v*e.stride+e.offset+k);if(bones.contains(bone))weights[v]+=Read<uint8_t>(buffer,we.start+v*we.stride+we.offset+k)/255.0;}
                }
            }
            const auto& indices=chunk.at("chunkIndices");const std::string format=indices.at("pe");const size_t width=format=="IBCT_IndexUShort"?2:format=="IBCT_IndexUInt"?4:0;
            Require(width!=0,"unsupported index format "+format);const size_t at=header.at("indexBufferOffset").get<size_t>()+indices.at("teOffset").get<size_t>();
            std::map<uint32_t,uint32_t> remap;
            for(uint32_t t=0;t<ni;t+=3){T tri{};bool keep=true;for(int k=0;k<3;++k){const size_t p=at+size_t(t+k)*width;tri[k]=width==2?Read<uint16_t>(buffer,p):Read<uint32_t>(buffer,p);Require(tri[k]<nv,"invalid triangle index");keep&=weights[tri[k]]>.5;}
                if(!keep)continue;for(auto& vertex:tri){auto it=remap.find(vertex);if(it==remap.end()){const auto next=uint32_t(result.positions.size());
                        result.positions.push_back(Transform(pos[vertex],frame));result.normals.push_back(Normal(norm[vertex],normalMatrix));it=remap.emplace(vertex,next).first;}vertex=it->second;}
                result.triangles.push_back(tri);
            }
        }
    }
    size_t reversed=0;for(const auto& t:result.triangles){const V face=Cross(Minus(result.positions[t[1]],result.positions[t[0]]),Minus(result.positions[t[2]],result.positions[t[0]]));
        reversed+=Dot(face,Plus(Plus(result.normals[t[0]],result.normals[t[1]]),result.normals[t[2]]))<0;}
    if(reversed>result.triangles.size()/2)for(auto& t:result.triangles)std::swap(t[1],t[2]);
    using Key=std::array<int64_t,3>;std::vector<Key> keys;std::map<Key,V> sums;
    for(size_t i=0;i<result.positions.size();++i){Key key{};for(int k=0;k<3;++k)key[k]=std::llround(result.positions[i][k]/1e-5);keys.push_back(key);sums[key]=Plus(sums[key],result.normals[i]);}
    for(size_t i=0;i<result.positions.size();++i){const V sum=sums.at(keys[i]);const double length=std::sqrt(Dot(sum,sum));Require(std::isfinite(length),"invalid welded normal");
        // Coincident front/back vertices of a thin, two-sided part can cancel.
        // Keep that point in place rather than choosing an arbitrary side.
        if(length>1e-9)result.positions[i]=Plus(result.positions[i],Times(sum,inflate/length));}
    return result;
}
V Gltf(V v){return {v[0],v[2],-v[1]};}
std::pair<V,V> Bounds(const std::vector<V>& positions){Require(!positions.empty(),"empty geometry");V lo=positions[0],hi=lo;
    for(const auto& p:positions)for(int k=0;k<3;++k){Require(std::isfinite(p[k]),"nonfinite vertex");lo[k]=std::min(lo[k],p[k]);hi[k]=std::max(hi[k],p[k]);}return {lo,hi};}
Bytes Glb(const Geometry& geometry,const std::string& name){
    Bytes buffer;J views=J::array(),accessors=J::array();std::vector<V> positions;for(auto p:geometry.positions)positions.push_back(Gltf(p));const auto [lo,hi]=Bounds(positions);
    const auto add=[&](const std::vector<double>& values,int components,const char* type){const size_t start=buffer.size();for(double x:values)Add(buffer,float(x));
        views.push_back({{"buffer",0},{"byteOffset",start},{"byteLength",buffer.size()-start},{"target",34962}});accessors.push_back({{"bufferView",views.size()-1},{"componentType",5126},{"count",values.size()/components},{"type",type}});};
    std::vector<double> values;for(const auto& p:positions)values.insert(values.end(),p.begin(),p.end());add(values,3,"VEC3");accessors[0]["min"]=lo;accessors[0]["max"]=hi;
    values.clear();for(auto n:geometry.normals){n=Gltf(n);values.insert(values.end(),n.begin(),n.end());}add(values,3,"VEC3");
    values.clear();for(auto n:geometry.normals){const V axis=std::abs(n[2])<.9?V{0,0,1}:V{1,0,0};auto t=Gltf(Unit(Cross(n,axis)));values.insert(values.end(),t.begin(),t.end());values.push_back(1);}add(values,4,"VEC4");
    values.clear();for(size_t i=0;i<positions.size();++i){values.push_back(0);values.push_back(1);}add(values,2,"VEC2");
    const size_t indexStart=buffer.size();for(const auto& t:geometry.triangles)for(uint32_t i:t)Add(buffer,i);
    views.push_back({{"buffer",0},{"byteOffset",indexStart},{"byteLength",buffer.size()-indexStart},{"target",34963}});
    accessors.push_back({{"bufferView",4},{"componentType",5125},{"count",geometry.triangles.size()*3},{"type","SCALAR"}});
    const J primitive{{"attributes",{{"POSITION",0},{"NORMAL",1},{"TANGENT",2},{"TEXCOORD_0",3}}},{"indices",4},{"material",0}};
    const J doc{{"asset",{{"version","2.0"},{"generator","CyberpunkVRPort C++ glow parts"}}},{"scene",0},{"scenes",J::array({{{"name",name},{"nodes",J::array({0})}}})},
        {"nodes",J::array({{{"name","submesh_00_LOD_1"},{"mesh",0}}})},{"meshes",J::array({{{"name","submesh_00_LOD_1"},{"primitives",J::array({primitive})}}})},
        {"materials",J::array({{{"name","glow"}}})},{"buffers",J::array({{{"byteLength",buffer.size()}}})},{"bufferViews",views},{"accessors",accessors}};
    auto json=doc.dump();while(json.size()%4)json+=' ';Bytes out;Add(out,uint32_t(0x46546C67));Add(out,uint32_t(2));Add(out,uint32_t(28+json.size()+buffer.size()));
    Add(out,uint32_t(json.size()));Add(out,uint32_t(0x4E4F534A));out.insert(out.end(),json.begin(),json.end());Add(out,uint32_t(buffer.size()));Add(out,uint32_t(0x004E4942));out.insert(out.end(),buffer.begin(),buffer.end());return out;
}
void SelfTest(){
    Bytes bytes;for(int v=0;v<6;++v){const V p{v<3?.2+(v==1?.02:0):.6+(v==4?.02:0),v%3==2?.02:0,0};for(auto x:p)Add(bytes,int16_t(std::lround(x*32767)));Add(bytes,int16_t(32767));
        Add(bytes,uint32_t(v<3?1:0));Add(bytes,uint32_t(255));}
    const auto normalStart=bytes.size();for(int v=0;v<6;++v)Add(bytes,uint32_t(512|(512<<10)|(1023<<20)));const auto indexStart=bytes.size();for(uint16_t i=0;i<6;++i)Add(bytes,i);
    J matrix;const char* axes[]{"X","Y","Z","W"};for(int i=0;i<4;++i)for(int j=0;j<4;++j)matrix[axes[i]][axes[j]]=i==j?1.:0.;matrix["W"]["X"]=-.2;
    J elements=J::array();for(const auto& pair:std::array<std::array<const char*,2>,4>{{{"PS_Position","PT_Short4N"},{"PS_SkinIndices","PT_UByte4"},{"PS_SkinWeights","PT_UByte4N"},{"PS_Normal","PT_Dec4"}}})
        elements.push_back({{"usage",pair[0]},{"type",pair[1]},{"streamIndex",std::string(pair[0])=="PS_Normal"?1:0}});
    elements.push_back({{"usage","PS_InstanceTransform"},{"type","PT_Float4"},{"streamIndex",7},{"streamType","ST_PerInstance"}});
    J chunk{{"numVertices",6},{"numIndices",6},{"lodMask",1},{"chunkVertices",{{"byteOffsets",{{"Elements",J::array({0,normalStart})}}},{"vertexLayout",{{"elements",{{"Elements",elements}}}}}}},
        {"chunkIndices",{{"pe","IBCT_IndexUShort"},{"teOffset",0}}}};
    J mesh{{"boneNames",J::array({"bolt","handle"})},{"boneRigMatrices",J::array({matrix,matrix})},{"renderResourceBlob",{{"Data",{{"header",{{"quantizationScale",{{"X",1},{"Y",1},{"Z",1}}},
        {"quantizationOffset",{{"X",0},{"Y",0},{"Z",0}}},{"indexBufferOffset",indexStart},{"renderChunkInfos",J::array({chunk})}}},{"renderBuffer",{{"Bytes",Encode(bytes)}}}}}}}};
    const auto part=Generate({mesh},"handle",{"handle"},.0015),whole=Generate({mesh},"handle",{"bolt","handle"},0);
    Require(part.triangles.size()==1&&part.positions.size()==3&&whole.triangles.size()==2,"part selection included the bolt body");const auto [lo,hi]=Bounds(part.positions);
    Require(std::abs(lo[0])<.00005&&hi[0]<.021&&std::abs(lo[2]-.0015)<.00001,"bone-frame transform or inflation failed");
    Bytes opposite=bytes;for(int v=0;v<6;++v){const uint32_t normal=(~Read<uint32_t>(bytes,normalStart+4*v))&0x3fffffff;std::memcpy(opposite.data()+normalStart+4*v,&normal,4);}
    for(int t=0;t<2;++t)for(int k=0;k<3;++k){const uint16_t index=uint16_t(3*t+2-k);std::memcpy(opposite.data()+indexStart+2*(3*t+k),&index,2);}
    auto back=mesh;back["renderResourceBlob"]["Data"]["renderBuffer"]["Bytes"]=Encode(opposite);
    const auto thin=Generate({mesh,back},"handle",{"handle"},.0015);Require(thin.triangles.size()==2&&std::abs(thin.positions[0][2])<1e-8,"two-sided welded shell picked an arbitrary normal");
    auto binary=Glb(part,"test");Require(Read<uint32_t>(binary,8)==binary.size(),"invalid GLB length");const auto jsonBytes=Read<uint32_t>(binary,12);
    const auto doc=J::parse(binary.begin()+20,binary.begin()+20+jsonBytes);Require(doc["accessors"][0]["count"]==3&&doc["accessors"][4]["count"]==3,"invalid GLB accessors");
    std::cout<<"selected-bone triangles, frame transform, welded shell and GLB structure PASS\n";
}
int main(int argc,char** argv){try{
    if(argc==2&&std::string(argv[1])=="--self-test"){SelfTest();return 0;}
    Require(argc==4,"usage: reload_glow_part PLAN.json OUTPUT.glb REPORT.json");const std::filesystem::path planPath=argv[1];const auto plan=Load(planPath);
    std::vector<J> meshes;for(const auto& p:plan.at("meshes")){std::filesystem::path path=p.get<std::string>();if(path.is_relative())path=planPath.parent_path()/path;meshes.push_back(Load(path));}
    const auto frame=plan.at("frame").get<std::string>();const auto selected=plan.at("bones").get<std::set<std::string>>();const double inflate=plan.value("inflate",.0015);
    Require(inflate>=0&&inflate<=.01,"invalid shell thickness");const auto g=Generate(meshes,frame,selected,inflate);
    J report{{"frame",frame},{"bones",selected},{"vertices",g.positions.size()},{"triangles",g.triangles.size()},{"sourceTriangles",g.sourceTriangles},{"inflate",inflate}};
    if(g.positions.empty()){report["status"]="no_geometry";std::ofstream(argv[3])<<report.dump(2)<<'\n';std::cout<<report.dump()<<'\n';return 3;}
    const auto [lo,hi]=Bounds(g.positions);report["min"]=lo;report["max"]=hi;report["status"]="ok";
    const auto bytes=Glb(g,plan.value("name",std::string("glow")));std::ofstream out(argv[2],std::ios::binary);out.write(reinterpret_cast<const char*>(bytes.data()),bytes.size());Require(bool(out),"cannot write GLB");
    std::ofstream summary(argv[3]);summary<<report.dump(2)<<'\n';Require(bool(summary),"cannot write report");std::cout<<report.dump()<<'\n';return 0;
}catch(const std::exception& e){std::cerr<<e.what()<<'\n';return 1;}}
