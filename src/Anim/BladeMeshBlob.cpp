#include "Anim/BladeMeshBlob.hpp"
#include <RED4ext/Scripting/Natives/CMesh.hpp>
#include <RED4ext/Scripting/Natives/Generated/mesh/MeshAppearance.hpp>
#include <RED4ext/Scripting/Natives/Generated/rend/RenderMeshBlob.hpp>
#include <DirectXPackedVector.h>
#include <algorithm>
#include <cmath>
#include <cstring>
#include <limits>
#include <unordered_map>

namespace cvr::blade::mesh {
namespace {
using namespace RED4ext;
using namespace GpuWrapApi::VertexPacking;
using Bytes=std::vector<uint8_t>;
using Channels=std::array<Bytes,5>;
struct Attr { ePackingUsage usage; ePackingType type; unsigned bank,stream,offset,size; };
struct Layout {
    std::array<unsigned,5> stride{};
    std::vector<Attr> attrs;
    int position{-1}, shadingNormal{-1};
    int bones[2]{-1,-1}, weights[2]{-1,-1};
};
unsigned Width(ePackingType type) {
    switch(type) {
    case ePackingType::PT_Short4N: case ePackingType::PT_Float16_4: return 8;
    case ePackingType::PT_UByte4: case ePackingType::PT_UByte4N: case ePackingType::PT_Dec4:
    case ePackingType::PT_Color: case ePackingType::PT_Float16_2: return 4;
    default:return 0;
    }
}
bool ReadLayout(const rend::Chunk& chunk,Layout& layout) {
    const auto& desc=chunk.chunkVertices.vertexLayout;
    for(unsigned elemIndex=0;elemIndex<desc.elements.size();++elemIndex) {
        const auto& elem=desc.elements[elemIndex];
        if(elem.type==ePackingType::PT_Invalid) break;
        if(elem.streamType==EStreamType::ST_PerInstance) continue;
        unsigned stream=elem.streamIndex, size=Width(elem.type);
        if(elem.streamType!=EStreamType::ST_PerVertex || stream>=5 || !size) return false;
        const int index=static_cast<int>(layout.attrs.size());
        if(elem.usage==ePackingUsage::PS_Normal && elem.type==ePackingType::PT_Dec4) layout.shadingNormal=index;
        if(elem.usage==ePackingUsage::PS_Position) {
            if(elem.type!=ePackingType::PT_Short4N || layout.position>=0) return false;
            layout.position=index;
        } else if(elem.usage==ePackingUsage::PS_SkinIndices || elem.usage==ePackingUsage::PS_SkinWeights) {
            if(elem.usageIndex>1) return false;
            bool ids=elem.usage==ePackingUsage::PS_SkinIndices;
            if(elem.type!=(ids?ePackingType::PT_UByte4:ePackingType::PT_UByte4N)) return false;
            (ids?layout.bones:layout.weights)[elem.usageIndex]=index;
        } else if(elem.type!=ePackingType::PT_Float16_2 && elem.type!=ePackingType::PT_Float16_4 &&
                  elem.type!=ePackingType::PT_Dec4 && elem.type!=ePackingType::PT_Color) return false;
        layout.attrs.push_back({elem.usage,elem.type,elem.usageIndex,stream,layout.stride[stream],size});
        layout.stride[stream]+=size;
    }
    for(size_t stream=0;stream<5;++stream) if(layout.stride[stream] && (stream>=desc.slotStrides.size() ||
        layout.stride[stream]!=desc.slotStrides[stream] || stream>=chunk.chunkVertices.byteOffsets.size())) return false;
    return layout.position>=0 && layout.bones[0]>=0 && layout.weights[0]>=0 &&
        ((layout.bones[1]>=0)==(layout.weights[1]>=0));
}
template<class T> T Read(const uint8_t* data) {T value; std::memcpy(&value,data,sizeof(value));return value;}
template<class T> void Write(uint8_t* data,T value) {std::memcpy(data,&value,sizeof(value));}
bool Within(size_t start,size_t length,size_t size) {return start<=size && length<=size-start;}
void Align(Bytes& bytes,size_t alignment) {bytes.resize((bytes.size()+alignment-1)&~(alignment-1));}
Point Apply(const Point& point,const Matrix& matrix) {
    return {matrix.W.X+point[0]*matrix.X.X+point[1]*matrix.Y.X+point[2]*matrix.Z.X,
            matrix.W.Y+point[0]*matrix.X.Y+point[1]*matrix.Y.Y+point[2]*matrix.Z.Y,
            matrix.W.Z+point[0]*matrix.X.Z+point[1]*matrix.Y.Z+point[2]*matrix.Z.Z};
}
struct StreamSet {
    Layout layout;
    Channels streams;
    unsigned count{};
    const uint8_t* Field(int field,unsigned vertex) const {
        const auto& attr=layout.attrs[field];
        return streams[attr.stream].data()+size_t(vertex)*layout.stride[attr.stream]+attr.offset;
    }
    bool Blend(const std::array<uint32_t,3>& triangle,const Point& bary,uint32_t& output) {
        for(unsigned index=0;index<3;++index) if(bary[index]>=1.-1e-10) {output=triangle[index];return true;}
        if(count>=65535) return false;
        output=count++;
        for(unsigned stream=0;stream<5;++stream) streams[stream].resize(size_t(count)*layout.stride[stream]);
        for(unsigned field=0;field<layout.attrs.size();++field) {
            const auto& attr=layout.attrs[field];
            auto* destination=streams[attr.stream].data()+size_t(output)*layout.stride[attr.stream]+attr.offset;
            if(attr.usage==ePackingUsage::PS_SkinIndices || attr.usage==ePackingUsage::PS_SkinWeights) continue;
            const bool half=attr.type==ePackingType::PT_Float16_2 || attr.type==ePackingType::PT_Float16_4;
            const bool packed=attr.type==ePackingType::PT_Dec4;
            const bool position=attr.usage==ePackingUsage::PS_Position;
            uint32_t bits=0;
            for(unsigned channel=0;channel<(half?attr.size/2:4);++channel) {
                double value=0;
                for(unsigned index=0;index<3;++index) {
                    const auto* data=Field(field,triangle[index]);
                    const int shift=10*channel;
                    const double term=half?DirectX::PackedVector::XMConvertHalfToFloat(Read<uint16_t>(data+channel*2)):
                        packed?double((Read<uint32_t>(data)>>shift)&(channel==3?3:1023)):
                        position?double(Read<int16_t>(data+channel*2)):double(data[channel]);
                    value+=term*bary[index];
                }
                if(!std::isfinite(value)) return false;
                if(half) Write(destination+channel*2,DirectX::PackedVector::XMConvertFloatToHalf(float(value)));
                else if(position) Write(destination+channel*2,static_cast<int16_t>(std::lround(value)));
                else if(packed) bits|=static_cast<uint32_t>(std::lround(value))<<(10*channel);
                else destination[channel]=static_cast<uint8_t>(std::lround(value));
            }
            if(packed) Write(destination,bits);
        }
        std::array<double,256> totals{};
        const int banks=layout.bones[1]>=0?2:1;
        for(unsigned vertex=0;vertex<3;++vertex) for(int bank=0;bank<banks;++bank) {
            const auto* ids=Field(layout.bones[bank],triangle[vertex]);
            const auto* weights=Field(layout.weights[bank],triangle[vertex]);
            for(int channel=0;channel<4;++channel) totals[ids[channel]]+=bary[vertex]*weights[channel];
        }
        std::array<unsigned,8> bones{};std::array<double,8> weights{};
        double total=0;
        for(int index=0;index<4*banks;++index) {
            auto best=std::max_element(totals.begin(),totals.end());bones[index]=unsigned(best-totals.begin());
            weights[index]=*best;total+=*best;*best=0;
        }
        if(total<=0) return false;
        int allocated=0;double sum=0;
        for(int index=0;index<4*banks;++index) {
            sum+=weights[index];const auto next=static_cast<int>(std::lround(255*sum/total));
            const auto& indices=layout.attrs[layout.bones[index/4]];
            const auto& weight=layout.attrs[layout.weights[index/4]];
            streams[indices.stream][size_t(output)*layout.stride[indices.stream]+indices.offset+index%4]=uint8_t(bones[index]);
            streams[weight.stream][size_t(output)*layout.stride[weight.stream]+weight.offset+index%4]=uint8_t(next-allocated);
            allocated=next;
        }
        return true;
    }
};
struct Update { unsigned index{},count{},indices{},offset{},inkCount{},inkOffset{};std::array<unsigned,5> starts{}; };
struct Projection {
    double distance[2]{1.,1.};
    std::array<double,3> fraction{2.,2.,2.};
    Triangle normal{};
    // Keep the ordinary stamp if available. The fallback is restricted to
    // the source chunk actually intersected at the skin-weight boundary.
    SurfaceHit stamp{},blendHit{};
    unsigned blendChunk{};
};
Point Endpoint(const Volume& volume,unsigned side) {
    Point position=volume.center;
    for(unsigned axis=0;axis<3;++axis)
        position[axis]+=volume.axes[0][axis]*(side?1.:-1.)*volume.extent[0]+volume.axes[2][axis]*(volume.extent[2]-volume.outside);
    return position;
}
Point DecodePoint(const uint8_t* pos,const rend::RenderMeshBlobHeader& header,const Matrix& bind) {
    const auto& scale=header.quantizationScale;const auto& offset=header.quantizationOffset;
    return Apply({Read<int16_t>(pos)*scale.X/32767.+offset.X,Read<int16_t>(pos+2)*scale.Y/32767.+offset.Y,
                  Read<int16_t>(pos+4)*scale.Z/32767.+offset.Z},bind);
}
Point DecodeNormal(const uint8_t* data,const Matrix& bind) {
    const auto packed=Read<uint32_t>(data);
    Point n{};
    for(unsigned k=0;k<3;++k) n[k]=2.*((packed>>(10*k))&1023)-1023.;
    return {n[0]*bind.X.X+n[1]*bind.Y.X+n[2]*bind.Z.X,
            n[0]*bind.X.Y+n[1]*bind.Y.Y+n[2]*bind.Z.Y,
            n[0]*bind.X.Z+n[1]*bind.Y.Z+n[2]*bind.Z.Z};
}
}

bool CutPrivateBlob(RED4ext::CMesh& mesh,const std::vector<Edit>& edits,Counts& counts) {
    using namespace RED4ext;
    auto base=mesh.renderResourceBlob;
    if(!base || base->GetType()->name!=rend::RenderMeshBlob::NAME || edits.empty() || edits.size()>12) return false;
    auto* blob=static_cast<rend::RenderMeshBlob*>(base.instance);
    auto& header=blob->header;auto& buffer=blob->renderBuffer.buffer;
    auto* data=static_cast<const uint8_t*>(buffer.data);
    constexpr size_t limit=32u*1024*1024;
    if(!data || buffer.size>limit || !Within(header.indexBufferOffset,header.indexBufferSize,buffer.size) ||
       header.vertexBufferSize>header.indexBufferOffset || header.indexBufferOffset+header.indexBufferSize!=buffer.size ||
       !header.renderChunks.IsEmpty() || !header.customData.IsEmpty() || !header.speedTreeWind.IsEmpty() ||
       !header.topologyData.IsEmpty() || !header.topologyMetadata.IsEmpty()) return false;
    for(const auto& topology:header.topology) if(!topology.data.IsEmpty() || !topology.metadata.IsEmpty()) return false;
    std::vector<std::array<Projection,8>> projections(edits.size());
    for(unsigned part=0;part<header.renderChunkInfos.size();++part) {
        const auto& chunk=header.renderChunkInfos[part];
        Layout layout;
        if(!ReadLayout(chunk,layout) || chunk.chunkIndices.pe!=GpuWrapApi::eIndexBufferChunkType::IBCT_IndexUShort ||
           chunk.numIndices%3 || !Within(chunk.chunkIndices.teOffset,size_t(chunk.numIndices)*2,header.indexBufferSize)) return false;
        for(unsigned stream=0;stream<5;++stream) if(layout.stride[stream] &&
            !Within(chunk.chunkVertices.byteOffsets[stream],size_t(chunk.numVertices)*layout.stride[stream],header.vertexBufferSize)) return false;
        const auto field=[&](int attr,unsigned vertex) {
            const auto& item=layout.attrs[attr];
            return data+chunk.chunkVertices.byteOffsets[item.stream]+size_t(vertex)*layout.stride[item.stream]+item.offset;
        };
        for(size_t editIndex=0;editIndex<edits.size();++editIndex) {
            const auto& edit=edits[editIndex];
            if(edit.mark!=Mark::None && (part>=64 || !(edit.visibleChunks&(uint64_t(1)<<part)))) continue;
            if(edit.bone<0 || size_t(edit.bone)>=mesh.boneRigMatrices.size()) return false;
            std::vector<Point> points(chunk.numVertices);
            std::vector<unsigned> weights(chunk.numVertices);
            const bool stamp=edit.mark==Mark::Slash || edit.mark==Mark::Puncture;
            const unsigned regionWeight=edit.swept?16u:90u;
            std::vector<Point> normals(stamp && layout.shadingNormal>=0 ? chunk.numVertices : 0);
            for(unsigned vertex=0;vertex<chunk.numVertices;++vertex) {
                for(int bank=0;bank<2 && layout.bones[bank]>=0;++bank) {
                    const auto* ids=field(layout.bones[bank],vertex); const auto* values=field(layout.weights[bank],vertex);
                    for(int ch=0;ch<4;++ch) if(edit.skinRegion[ids[ch]]) weights[vertex]+=values[ch];
                }
                if(stamp || weights[vertex]>=regionWeight) {
                    const auto& bind=mesh.boneRigMatrices[edit.bone];
                    points[vertex]=DecodePoint(field(layout.position,vertex),header,bind);
                    if(!normals.empty()) normals[vertex]=DecodeNormal(field(layout.shadingNormal,vertex),bind);
                }
            }
            const Point ends[]{Endpoint(edit.volume,0),Endpoint(edit.volume,1)};
            Point seed;for(unsigned k=0;k<3;++k)seed[k]=(ends[0][k]+ends[1][k])*.5;
            const auto rays=SampleSweep(edit.rays);
            for(unsigned index=0;index<chunk.numIndices;index+=3) {
                const auto* tri=data+header.indexBufferOffset+chunk.chunkIndices.teOffset+2*index;
                const unsigned ids[]{Read<uint16_t>(tri),Read<uint16_t>(tri+2),Read<uint16_t>(tri+4)};
                if(std::max({ids[0],ids[1],ids[2]})>=points.size()) return false;
                const auto weight=std::min({weights[ids[0]],weights[ids[1]],weights[ids[2]]});
                const bool blended=weight<regionWeight;
                if(blended && (!stamp || !(weights[ids[0]] || weights[ids[1]] || weights[ids[2]]))) continue;
                const Triangle face{points[ids[0]],points[ids[1]],points[ids[2]]};
                Point outward{};
                if(!normals.empty()) for(unsigned k=0;k<3;++k)
                    outward[k]=normals[ids[0]][k]+normals[ids[1]][k]+normals[ids[2]][k];
                for(unsigned lod=0;lod<8;++lod) if(chunk.lodMask&(1<<lod)) {
                    auto& measurement=projections[editIndex][lod];
                    if(stamp) {
                        if(!blended) {
                            LocateStamp(rays,seed,edit.volume.axes[2],face,measurement.stamp,edit.shellDepth,
                                        normals.empty()?nullptr:&outward);
                        } else if(!measurement.stamp.found) {
                            const Point skinWeights{double(weights[ids[0]]),double(weights[ids[1]]),double(weights[ids[2]])};
                            SurfaceHit candidate;
                            LocateStamp(rays,seed,edit.volume.axes[2],face,candidate,edit.shellDepth,
                                        normals.empty()?nullptr:&outward,&skinWeights);
                            if(candidate.exact && (!measurement.blendHit.found ||
                               candidate.seedDistanceSquared<measurement.blendHit.seedDistanceSquared)) {
                                measurement.blendHit=candidate;
                                measurement.blendChunk=part;
                            }
                        }
                    } else if(edit.swept) {
                        for(unsigned sample=0;sample<rays.size();++sample)
                            SegmentHit(rays[sample][0],rays[sample][1],face,measurement.fraction[sample],measurement.normal[sample]);
                    } else for(unsigned side=0;side<2;++side) {
                        Point bary;ProjectSurface(ends[side],edit.volume.axes[2],face,measurement.distance[side],bary);
                    }
                }
            }
        }
    }
    Bytes vertices(data,data+header.indexBufferOffset),indices(data+header.indexBufferOffset,data+buffer.size);
    std::vector<Update> updates;
    Counts stats{};
    double bestInkGap=std::numeric_limits<double>::infinity();
    for(unsigned part=0;part<header.renderChunkInfos.size();++part) {
        const auto& chunk=header.renderChunkInfos[part]; StreamSet set;
        if(!ReadLayout(chunk,set.layout) || chunk.chunkIndices.pe!=GpuWrapApi::eIndexBufferChunkType::IBCT_IndexUShort ||
           chunk.numIndices%3 || !Within(chunk.chunkIndices.teOffset,size_t(chunk.numIndices)*2,header.indexBufferSize)) return false;
        set.count=chunk.numVertices;
        for(unsigned stream=0;stream<5;++stream) if(auto stride=set.layout.stride[stream]) {
            const size_t start=chunk.chunkVertices.byteOffsets[stream],size=size_t(set.count)*stride;
            if(!Within(start,size,header.vertexBufferSize)) return false;
            set.streams[stream].assign(data+start,data+start+size);
        }
        std::vector<uint32_t> triangles(chunk.numIndices);
        std::vector<uint8_t> inkFaces(triangles.size()/3);
        for(size_t index=0;index<triangles.size();++index) {
            triangles[index]=Read<uint16_t>(data+header.indexBufferOffset+chunk.chunkIndices.teOffset+2*index);
            if(triangles[index]>=set.count) return false;
        }
        bool changed=false;
        std::vector<Point> inkCenters(edits.size()),inkNormals(edits.size());
        std::vector<bool> shellProjections(edits.size());
        for(size_t editIndex=0;editIndex<edits.size();++editIndex) {
            const auto& edit=edits[editIndex];
            const Projection* nearest=nullptr;
            if(edit.mark!=Mark::None && (part>=64 || !(edit.visibleChunks&(uint64_t(1)<<part)))) continue;
            for(unsigned lod=0;lod<8;++lod) if(chunk.lodMask&(1<<lod)) {nearest=&projections[editIndex][lod];break;}
            if(!nearest) continue;
            Cut projected{Endpoint(edit.volume,0),Endpoint(edit.volume,1),edit.volume.axes[2],
                          edit.volume.extent[1],2*edit.volume.extent[2]-edit.volume.outside,edit.volume.outside};
            const bool stamp=edit.mark==Mark::Slash || edit.mark==Mark::Puncture;
            const bool blendBoundary=stamp && !nearest->stamp.found;
            const auto& stampHit=blendBoundary?nearest->blendHit:nearest->stamp;
            if(stamp) {
                if(!stampHit.found || (blendBoundary && nearest->blendChunk!=part)) continue;
                projected.normal=stampHit.normal;
                Point tangent=edit.volume.axes[0];const auto& hit=stampHit.point;
                double dot=0;
                for(unsigned i=0;i<3;++i) {
                    dot+=tangent[i]*projected.normal[i];
                }
                for(unsigned i=0;i<3;++i) tangent[i]-=dot*projected.normal[i];
                double length=std::hypot(tangent[0],tangent[1],tangent[2]);
                if(length<.01) {
                    const auto& n=projected.normal;
                    tangent=std::abs(n[2])<.8?Point{-n[1],n[0],0}:Point{0,-n[2],n[1]};
                    length=std::hypot(tangent[0],tangent[1],tangent[2]);
                }
                const auto halfLength=edit.mark==Mark::Puncture?.008:.028;
                projected.halfWidth=edit.mark==Mark::Puncture?.005:.0025;
                projected.depth=.004;projected.outside=.004;
                for(unsigned i=0;i<3;++i) {
                    projected.from[i]=hit[i]-halfLength*tangent[i]/length;
                    projected.to[i]=hit[i]+halfLength*tangent[i]/length;
                }
            } else if(edit.swept) {
                if(!ResolveSweep(SampleSweep(edit.rays),nearest->fraction,nearest->normal,projected)) continue;
            } else {
                if(nearest->distance[0]==1. || nearest->distance[1]==1. ||
                    std::abs(nearest->distance[0]-nearest->distance[1])>2*edit.volume.extent[0]) continue;
                for(unsigned axis=0;axis<3;++axis) {
                    projected.from[axis]+=projected.normal[axis]*nearest->distance[0];
                    projected.to[axis]+=projected.normal[axis]*nearest->distance[1];
                }
            }
            Volume volume;
            if(!MakeVolume(projected,volume)) continue;
            for(unsigned k=0;k<3;++k) inkCenters[editIndex][k]=(projected.from[k]+projected.to[k])*.5;
            inkNormals[editIndex]=projected.normal;
            shellProjections[editIndex]=stamp && !stampHit.onSegment;
            if(edit.bone<0 || size_t(edit.bone)>=mesh.boneRigMatrices.size()) return false;
            std::vector<Point> points(set.count); std::vector<double> influence(set.count);
            for(unsigned vertex=0;vertex<set.count;++vertex) {
                const auto* pos=set.Field(set.layout.position,vertex);
                points[vertex]=DecodePoint(pos,header,mesh.boneRigMatrices[edit.bone]);
                for(int bank=0;bank<2 && set.layout.bones[bank]>=0;++bank) {
                    const auto* ids=set.Field(set.layout.bones[bank],vertex);
                    const auto* weights=set.Field(set.layout.weights[bank],vertex);
                    for(int channel=0;channel<4;++channel) if(edit.skinRegion[ids[channel]]) influence[vertex]+=weights[channel]/255.;
                }
            }
            std::vector<uint32_t> next;next.reserve(triangles.size());std::vector<Triangle> pieces,ink;
            std::vector<uint8_t> nextInk;nextInk.reserve(inkFaces.size());
            std::unordered_map<uint64_t,uint32_t> edgeVertices;
            const double requiredInfluence=edit.swept?16./255.:.35;
            for(size_t index=0;index<triangles.size();index+=3) {
                const std::array<uint32_t,3> face{triangles[index],triangles[index+1],triangles[index+2]};
                const Triangle position{points[face[0]],points[face[1]],points[face[2]]};
                std::array<double,3> faceWeights{influence[face[0]],influence[face[1]],influence[face[2]]};
                const bool eligible=blendBoundary ? *std::max_element(faceWeights.begin(),faceWeights.end())>0. :
                    *std::min_element(faceWeights.begin(),faceWeights.end())>=requiredInfluence;
                if(!eligible ||
                   !(edit.mark==Mark::None ? Subtract(position,volume,pieces) : MarkFace(position,volume,!stamp,pieces,ink,edit.mark==Mark::Puncture))) {
                    next.insert(next.end(),face.begin(),face.end());nextInk.push_back(inkFaces[index/3]);continue;
                }
                changed=true;++stats.split;
                nextInk.insert(nextInk.end(),pieces.size(),inkFaces[index/3]);
                if(edit.mark!=Mark::None) {
                    nextInk.insert(nextInk.end(),ink.size(),uint8_t(editIndex+1));
                    pieces.insert(pieces.end(),ink.begin(),ink.end());
                }
                std::vector<std::pair<Point,uint32_t>> interior;
                for(const auto& piece:pieces) for(const auto& bary:piece) {
                    uint32_t vertex{};
                    const auto other=std::min_element(bary.begin(),bary.end());
                    if(*std::max_element(bary.begin(),bary.end())>=1.-1e-10) {
                        vertex=face[std::max_element(bary.begin(),bary.end())-bary.begin()];
                    } else if(*other<1e-10) {
                        const unsigned opposite=unsigned(other-bary.begin());
                        unsigned low=(opposite+1)%3,high=(opposite+2)%3;
                        if(face[low]>face[high]) std::swap(low,high);
                        const auto ratio=uint32_t(std::llround(std::clamp(bary[high],0.,1.)*16777215.));
                        const uint64_t key=uint64_t(face[low])<<40|uint64_t(face[high])<<24|ratio;
                        auto [it,inserted]=edgeVertices.try_emplace(key);
                        if(inserted && !set.Blend(face,bary,it->second)) return false;
                        vertex=it->second;
                    } else {
                        const auto existing=std::find_if(interior.begin(),interior.end(),[&](const auto& item){
                            return std::abs(item.first[0]-bary[0])+std::abs(item.first[1]-bary[1])<1e-9;
                        });
                        if(existing!=interior.end()) vertex=existing->second;
                        else {
                            if(!set.Blend(face,bary,vertex)) return false;
                            interior.emplace_back(bary,vertex);
                        }
                    }
                    next.push_back(vertex);
                }
            }
            triangles.swap(next);
            inkFaces.swap(nextInk);
        }
        if(!changed) continue;
        // Later edits can split/remove earlier ink. Measure the surviving,
        // quantized triangle that will actually go into the painted index buffer.
        for(size_t i=0;i<inkFaces.size();++i) {
            if(!inkFaces[i]) continue;
            const int tag=inkFaces[i]-1;
            if(tag<stats.contactEdit || (tag==stats.contactEdit && chunk.lodMask>stats.contactLod)) continue;
            const auto& edit=edits[tag];
            Triangle face;
            for(unsigned k=0;k<3;++k) face[k]=DecodePoint(set.Field(set.layout.position,triangles[3*i+k]),
                                                          header,mesh.boneRigMatrices[edit.bone]);
            Point point,normal;
            if(!ClosestSurfacePoint(face,inkCenters[tag],point,normal)) continue;
            double gap=0.,orientation=0.,entryDistance=0.;
            const Point start=Endpoint(edit.volume,0),end=Endpoint(edit.volume,1);
            for(unsigned k=0;k<3;++k) {
                const double delta=point[k]-inkCenters[tag][k],fromEntry=point[k]-(start[k]+end[k])*.5;
                gap+=delta*delta;entryDistance+=fromEntry*fromEntry;
                orientation+=normal[k]*inkNormals[tag][k];
            }
            if(tag==stats.contactEdit && chunk.lodMask==stats.contactLod && gap>=bestInkGap) continue;
            if(orientation<0) for(auto& n:normal) n=-n;
            stats.contact=point;stats.normal=normal;stats.contactEdit=tag;stats.contactChunk=part;
            stats.contactLod=chunk.lodMask;stats.entryDistanceSquared=entryDistance;
            stats.projectedFromShell=shellProjections[tag];bestInkGap=gap;
        }
        Update update{part,set.count};
        for(unsigned stream=0;stream<5;++stream) if(set.layout.stride[stream]) {
            Align(vertices,16);update.starts[stream]=unsigned(vertices.size());
            vertices.insert(vertices.end(),set.streams[stream].begin(),set.streams[stream].end());
        }
        Align(indices,4);update.offset=unsigned(indices.size());
        auto writeFaces=[&](bool painted) {
            unsigned count=0;
            for(size_t i=0;i<triangles.size();++i) if(bool(inkFaces[i/3])==painted) {
                auto vertex=triangles[i];indices.push_back(uint8_t(vertex));indices.push_back(uint8_t(vertex>>8));++count;
            }
            return count;
        };
        update.indices=writeFaces(false);
        Align(indices,4);update.inkOffset=unsigned(indices.size());update.inkCount=writeFaces(true);
        stats.inkTriangles+=update.inkCount/3;
        if(vertices.size()+indices.size()>limit) return false;
        stats.indicesBefore+=chunk.numIndices;stats.indicesAfter+=update.indices+update.inkCount;stats.inserted+=set.count-chunk.numVertices;
        updates.push_back(update);
    }
    if(updates.empty()) {
        counts.noOverlap=true;
        return false;
    }
    const auto originalCount=header.renderChunkInfos.size();
    if(!header.topology.IsEmpty() && header.topology.size()!=originalCount) return false;
    const auto inkCount=std::count_if(updates.begin(),updates.end(),[](const Update& u){return u.inkCount!=0;});
    if(inkCount) {
        if(originalCount+inkCount>64 || mesh.appearances.IsEmpty() || mesh.externalMaterials.size()>=65535) return false;
        for(const auto& app:mesh.appearances) if(!app || app->chunkMaterials.size()!=originalCount) return false;
    }
    const auto vertexBytes=unsigned(vertices.size());Align(vertices,1024);
    const auto indexStart=unsigned(vertices.size());
    RawBuffer replacement;
    replacement.Initialize(buffer.GetAllocator(),uint32_t(vertices.size()+indices.size()),buffer.alignment);
    if(!replacement.data) return false;
    std::memcpy(replacement.data,vertices.data(),vertices.size());
    std::memcpy(static_cast<uint8_t*>(replacement.data)+vertices.size(),indices.data(),indices.size());
    const CName inkMaterial("vrport_surface_ink");
    // Optional precomputed ray-tracing opacity data indexes the original
    // triangles. The changed private mesh uses its ordinary material opacity.
    header.opacityMicromaps.Clear();
    if(inkCount) {
        CMeshMaterialEntry entry{};entry.name=inkMaterial;entry.index=uint16_t(mesh.externalMaterials.size());
        RaRef<IMaterial> inkRef{};inkRef.path=ResourcePath("vrport\\fx\\mark_core.mi");
        mesh.externalMaterials.PushBack(inkRef);mesh.materialEntries.PushBack(entry);
    }
    for(const auto& change:updates) {
        auto& chunk=header.renderChunkInfos[change.index];
        chunk.numVertices=uint16_t(change.count);chunk.numIndices=change.indices;chunk.chunkIndices.teOffset=change.offset;
        for(unsigned stream=0;stream<5;++stream) if(change.starts[stream]) chunk.chunkVertices.byteOffsets[stream]=change.starts[stream];
        if(change.inkCount) {
            auto painted=chunk;
            painted.numIndices=change.inkCount;painted.chunkIndices.teOffset=change.inkOffset;
            painted.materialId.Clear();
            stats.inkChunks.emplace_back(change.index,header.renderChunkInfos.size());
            if(stats.contactEdit>=0 && change.index==stats.contactChunk)
                stats.contactInkChunk=header.renderChunkInfos.size();
            header.renderChunkInfos.PushBack(painted);
            if(!header.topology.IsEmpty()) header.topology.PushBack(rend::TopologyData{});
            for(auto& app:mesh.appearances) app->chunkMaterials.PushBack(inkMaterial);
        }
    }
    header.vertexBufferSize=vertexBytes; header.indexBufferSize=uint32_t(indices.size()); header.indexBufferOffset=indexStart;
    buffer.Swap(replacement);counts=stats;return true;
}
}
