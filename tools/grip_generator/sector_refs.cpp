#include "cr2w_mesh.hpp"
#include <iostream>
#include <unordered_map>
using namespace gripgen;
static uint64_t Hash(std::string path){
    uint64_t hash=0xcbf29ce484222325ull;
    for(unsigned char c:path){if(c=='/')c='\\';if(c>='A'&&c<='Z')c+=32;hash=(hash^c)*0x100000001b3ull;}
    return hash;
}
static Bytes Block(std::ifstream& file,uint64_t offset,size_t size){
    Require(size<512u*1024*1024,"oversized archive block");Bytes data(size);file.clear();file.seekg(std::streamoff(offset));
    Require(bool(file.read(reinterpret_cast<char*>(data.data()),std::streamsize(size))),"cannot read archive block");return data;
}
int main(int argc,char** argv){
    try{
        Require(argc==6,"usage: sector_refs OODLE_DLL ARCHIVE_FOLDER SECTOR_NAMES TARGET_DEPOT_PATH OUTPUT_JSONL");
        Oodle oodle(argv[1]);std::ifstream names(argv[3]);Require(bool(names),"cannot read sector names");
        std::unordered_map<uint64_t,std::string> sectors;std::string line;
        while(std::getline(names,line)){if(!line.empty()&&line.back()=='\r')line.pop_back();if(line.ends_with(".streamingsector"))sectors[Hash(line)]=line;}
        const std::string target=argv[4];const auto hash=Hash(target);std::array<uint8_t,8> needle{};std::memcpy(needle.data(),&hash,8);
        std::ofstream output(argv[5]);Require(bool(output),"cannot create reference list");size_t inspected=0,found=0;
        for(const auto& item:std::filesystem::directory_iterator(argv[2])){
            if(item.path().extension()!=L".archive")continue;std::ifstream file(item.path(),std::ios::binary);
            const auto header=Block(file,0,40);Require(Read<uint32_t>(header,0)==0x52414452,"not an RDAR archive");
            const auto index=Block(file,Read<uint64_t>(header,8),Read<uint32_t>(header,16));
            const auto files=Read<uint32_t>(index,16),segments=Read<uint32_t>(index,20);const size_t segmentTable=28+size_t(files)*56;
            for(uint32_t i=0;i<files;++i){
                const size_t at=28+size_t(i)*56;const uint64_t entryHash=Read<uint64_t>(index,at);const auto name=sectors.find(entryHash);if(name==sectors.end())continue;
                const auto first=Read<uint32_t>(index,at+20);Require(first<segments,"archive segment out of range");const size_t segment=segmentTable+size_t(first)*16;
                const auto compressed=Block(file,Read<uint64_t>(index,segment),Read<uint32_t>(index,segment+8));
                const auto raw=oodle.Unpack(compressed,Read<uint32_t>(index,segment+12));
                const bool named=std::search(raw.begin(),raw.end(),target.begin(),target.end())!=raw.end();
                const bool hashed=std::search(raw.begin(),raw.end(),needle.begin(),needle.end())!=raw.end();
                if(named||hashed){output<<Json{{"archive",item.path().string()},{"path",name->second},{"path_reference",named},{"hash_reference",hashed}}.dump()<<'\n';++found;output.flush();}
                if(++inspected%1000==0)std::cout<<inspected<<" sectors, "<<found<<" matches\n"<<std::flush;
            }
        }
        std::cout<<"complete: "<<inspected<<" sectors, "<<found<<" matches\n";return 0;
    }catch(const std::exception& e){std::cerr<<e.what()<<'\n';return 1;}
}
