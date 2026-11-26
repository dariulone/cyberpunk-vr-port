#pragma once
#include "cr2w_mesh.hpp"
namespace gripgen {
// A check against an independently exported WolvenKit GLB. Conversion from
// glTF's Y-up coordinates to the game's Z-up coordinates also stays in C++.
inline Json VerifyGlb(const Mesh& mesh,const std::filesystem::path& path){
    const auto file=Load(path);Require(Read<uint32_t>(file,0)==0x46546c67&&Read<uint32_t>(file,4)==2,"not glTF 2 GLB");
    Require(Read<uint32_t>(file,16)==0x4e4f534a,"GLB JSON chunk missing");const auto jsonSize=Read<uint32_t>(file,12);
    Require(size_t(jsonSize)+28<=file.size(),"truncated GLB JSON");
    const auto doc=Json::parse(file.begin()+20,file.begin()+20+jsonSize);
    const size_t binHeader=20+jsonSize;Require(Read<uint32_t>(file,binHeader+4)==0x004e4942,"GLB BIN chunk missing");
    const auto bytes=Read<uint32_t>(file,binHeader);Require(bytes<=file.size()-binHeader-8,"truncated GLB buffer");
    const auto binary=std::span<const uint8_t>(file).subspan(binHeader+8,bytes);
    size_t vertices=0,triangles=0,reversed=0;double worst=0;
    for(const auto& part:doc.at("meshes"))for(const auto& primitive:part.at("primitives")){
        Require(primitive.value("mode",4)==4,"reference GLB primitive is not triangles");
        const auto& access=doc.at("accessors").at(primitive.at("attributes").at("POSITION").get<size_t>());
        Require(access.at("componentType")==5126&&access.at("type")=="VEC3"&&!access.contains("sparse"),"unsupported GLB reference positions");
        const auto& view=doc.at("bufferViews").at(access.at("bufferView").get<size_t>());
        const size_t start=view.value("byteOffset",size_t(0))+access.value("byteOffset",size_t(0));
        const size_t count=access.at("count").get<size_t>(),stride=view.value("byteStride",size_t(12));
        Require(vertices+count<=mesh.vertices.size(),"reference has more vertices");const auto base=uint32_t(vertices);
        for(size_t i=0;i<count;++i){
            const auto& actual=mesh.vertices[vertices++];const size_t at=start+i*stride;
            const double dx=actual.x-Read<float>(binary,at),dy=actual.y+Read<float>(binary,at+8),dz=actual.z-Read<float>(binary,at+4);
            worst=std::max(worst,std::sqrt(dx*dx+dy*dy+dz*dz));
        }
        const auto& ia=doc.at("accessors").at(primitive.at("indices").get<size_t>());
        const auto& iv=doc.at("bufferViews").at(ia.at("bufferView").get<size_t>());
        const auto component=ia.at("componentType").get<unsigned>();const size_t width=component==5123?2:component==5125?4:0;
        Require(width!=0,"unsupported GLB reference indices");const size_t indexStart=iv.value("byteOffset",size_t(0))+ia.value("byteOffset",size_t(0));
        const size_t indexCount=ia.at("count").get<size_t>();Require(indexCount%3==0,"invalid GLB reference triangles");
        for(size_t i=0;i<indexCount;i+=3){
            std::array<uint32_t,3> tri{};for(size_t k=0;k<3;++k){const size_t at=indexStart+(i+k)*width;tri[k]=base+(width==2?Read<uint16_t>(binary,at):Read<uint32_t>(binary,at));}
            if(tri[0]==tri[1]||tri[0]==tri[2]||tri[1]==tri[2])continue;
            Require(triangles<mesh.triangles.size(),"reference has more triangles");const auto& actual=mesh.triangles[triangles++];
            const auto cyclic=[](const auto& a,const auto& b){
                for(size_t shift=0;shift<3;++shift)if(a[0]==b[shift]&&a[1]==b[(shift+1)%3]&&a[2]==b[(shift+2)%3])return true;
                return false;
            };
            if(cyclic(tri,actual))continue;std::swap(tri[1],tri[2]);Require(cyclic(tri,actual),"reference triangle topology differs");++reversed;
        }
    }
    Require(vertices==mesh.vertices.size()&&triangles==mesh.triangles.size(),"reference geometry counts differ");
    Require(worst<1e-6,"reference vertex positions differ by more than 1 micron");
    return {{"vertices_checked",vertices},{"triangles_checked",triangles},{"reversed_winding",reversed},{"max_position_error_m",worst}};
}
}
