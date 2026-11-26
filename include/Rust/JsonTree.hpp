#pragma once
// jsoncpp's value tree lent to Rust flattened (rust/crates/cvr-render/src/jsoncpp_tree.rs): the text stays jsoncpp's to
// read, each caller's reader configured as before, and the rules that ask things of the tree live in Rust.
#include <json/json.h>
#include <cstddef>
#include <cstdint>
#include <vector>

namespace cvr::json_tree {
struct Str {const char* p;size_t n;};
// kind 0 null, 1 bool (value), 2 an integer that is not a u32, 3 an integer that is (value), 4 a real that is not an
// integral u32, 5 a string (text), 6 an array and 7 an object (count children from first, an object's carrying their
// name), 8 a real that is an integral u32 (value).
struct Node {uint32_t kind,count;uint64_t value;Str text,name;uint64_t first;};
static_assert(sizeof(Node)==56,"mirrored by jsoncpp_tree.rs");
// The tree under `value` into nodes[at] (nodes[0] is the root: start from a vector of one node).
inline void Flatten(const Json::Value& value,std::vector<Node>& nodes,size_t at){
    Node node{};
    switch(value.type()){
    case Json::nullValue:break;
    case Json::booleanValue:node.kind=1;node.value=value.asBool();break;
    case Json::intValue:case Json::uintValue:node.kind=value.isUInt()?3:2;node.value=value.isUInt()?value.asUInt():0;break;
    case Json::realValue:node.kind=value.isUInt()?8:4;node.value=value.isUInt()?value.asUInt():0;break;
    case Json::stringValue:{node.kind=5;const char* begin{};const char* end{};value.getString(&begin,&end);node.text={begin,size_t(end-begin)};break;}
    case Json::arrayValue:case Json::objectValue:{
        node.kind=value.isArray()?6:7;node.count=value.size();node.first=nodes.size();nodes.resize(nodes.size()+node.count);nodes[at]=node;
        size_t index=size_t(node.first);
        for(auto it=value.begin();it!=value.end();++it,++index){
            Flatten(*it,nodes,index);
            if(node.kind==7){const char* end{};const char* begin=it.memberName(&end);nodes[index].name={begin,size_t(end-begin)};}
        }
        return;}
    }
    nodes[at]=node;
}
}
