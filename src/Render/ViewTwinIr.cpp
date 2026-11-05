// ViewTwinIr -- see include/Render/ViewTwinIr.hpp.
#include "Render/ViewTwinIr.hpp"
#include <algorithm>
#include <cctype>
#include <charconv>
#include <map>
#include <set>
#include <string_view>
#include <unordered_map>
#include <vector>

namespace cvr::single_pass {
namespace {
using sv=std::string_view;
struct Fail {std::string why;};
[[noreturn]] void Throw(std::string why){throw Fail{std::move(why)};}

bool StartsWith(sv s,sv p){return s.size()>=p.size() && s.compare(0,p.size(),p)==0;}
sv Trim(sv s){while(!s.empty() && (s.front()==' ' || s.front()=='\t' || s.front()=='\r'))s.remove_prefix(1);
              while(!s.empty() && (s.back()==' ' || s.back()=='\t' || s.back()=='\r'))s.remove_suffix(1);return s;}
bool IsNameChar(char c){return std::isalnum(static_cast<unsigned char>(c)) || c=='.' || c=='_' || c=='-' || c=='$';}
uint32_t Number(sv s) {
    s=Trim(s);uint32_t v{};
    const auto r=std::from_chars(s.data(),s.data()+s.size(),v);
    if(r.ec!=std::errc() || r.ptr!=s.data()+s.size())Throw("not a number: "+std::string(s));
    return v;
}
uint64_t Number64(sv s) {
    s=Trim(s);uint64_t v{};
    const auto r=std::from_chars(s.data(),s.data()+s.size(),v);
    if(r.ec!=std::errc() || r.ptr!=s.data()+s.size())Throw("not a number: "+std::string(s));
    return v;
}
// "a, b, {c, d}, !"x, y"" -> top-level fields
std::vector<sv> Fields(sv s) {
    std::vector<sv> out;int depth=0;bool quote=false;size_t start=0;
    for(size_t i=0;i<s.size();++i) {
        const char c=s[i];
        if(quote){if(c=='"')quote=false;continue;}
        if(c=='"')quote=true;
        else if(c=='{' || c=='[' || c=='(' || c=='<')++depth;
        else if(c=='}' || c==']' || c==')' || c=='>')--depth;
        else if(c==',' && !depth){out.push_back(Trim(s.substr(start,i-start)));start=i+1;}
    }
    out.push_back(Trim(s.substr(start)));
    return out;
}
// "!12 = !{...}" -> 12 and the inside of the braces
bool MetadataNode(sv line,uint32_t& id,sv& body) {
    if(!StartsWith(line,"!") || line.size()<2 || !std::isdigit(static_cast<unsigned char>(line[1])))return false;
    const auto eq=line.find(" = !{");
    if(eq==sv::npos || line.back()!='}')return false;
    id=Number(line.substr(1,eq-1));
    body=line.substr(eq+5,line.size()-eq-6);
    return true;
}
uint32_t NodeRef(sv field) {   // "!12" -> 12
    field=Trim(field);
    if(!StartsWith(field,"!"))Throw("expected a metadata reference: "+std::string(field));
    return Number(field.substr(1));
}
// "  %12 = rest" -> "%12", rest; false when the line defines nothing
bool Definition(sv line,sv& name,sv& rest) {
    if(!StartsWith(line,"  %"))return false;
    size_t i=3;while(i<line.size() && IsNameChar(line[i]))++i;
    if(!StartsWith(line.substr(i)," = "))return false;
    name=line.substr(2,i-2);rest=line.substr(i+3);
    return true;
}
// every "%name" token on the line (types included; callers compare against value names only)
template<class F> void Tokens(sv line,F&& each) {
    for(size_t i=0;i<line.size();++i) {
        if(line[i]==';')return;   // comment
        if(line[i]=='"'){const auto e=line.find('"',i+1);if(e==sv::npos)return;i=e;continue;}
        if(line[i]!='%')continue;
        size_t j=i+1;while(j<line.size() && IsNameChar(line[j]))++j;
        if(j>i+1)each(line.substr(i,j-i));
        i=j-1;
    }
}
sv CodePart(sv line) {   // the instruction without its trailing "  ; comment"
    bool quote=false;
    for(size_t i=0;i<line.size();++i){if(line[i]=='"')quote=!quote;else if(line[i]==';' && !quote)return Trim(line.substr(0,i));}
    return Trim(line);
}
const char* ElementType(sv retType) {
    if(retType=="%dx.types.CBufRet.f32")return "float";
    if(retType=="%dx.types.CBufRet.i32")return "i32";
    if(retType=="%dx.types.CBufRet.f16")return "half";
    if(retType=="%dx.types.CBufRet.i16")return "i16";
    if(retType=="%dx.types.CBufRet.f64")return "double";
    if(retType=="%dx.types.CBufRet.i64")return "i64";
    return nullptr;
}
struct Cbv {uint32_t node,id,space,reg,count,size;std::string type,ext;};
// A signature element record: !{i32 id, !"name", i8 comp, i8 kind, !indices, i8 interp, i32 rows, i8 cols, i32 row, i8 col, ext}
struct Element {uint32_t node{},id{},kind{},rows{},row{},col{};std::string name,indices,fields[11];};
struct Module {
    std::vector<sv> lines;
    std::unordered_map<uint32_t,size_t> node;
    uint32_t maxNode{};
    explicit Module(const std::string& text) {
        sv all(text);size_t start=0;
        for(size_t i=0;i<all.size();++i)if(all[i]=='\n'){lines.push_back(all.substr(start,i-start));start=i+1;}
        if(start<all.size())lines.push_back(all.substr(start));
        for(auto& l:lines)if(!l.empty() && l.back()=='\r')l.remove_suffix(1);
        for(size_t i=0;i<lines.size();++i) {
            const sv l=lines[i];uint32_t id;sv body;
            if(MetadataNode(l,id,body)){node[id]=i;maxNode=std::max(maxNode,id);continue;}
            if(l.size()>1 && l[0]=='!' && std::isdigit(static_cast<unsigned char>(l[1]))) {
                const auto eq=l.find(" = ");
                if(eq!=sv::npos)maxNode=std::max(maxNode,Number(l.substr(1,eq-1)));
            }
        }
    }
    sv Body(uint32_t id)const {
        const auto it=node.find(id);if(it==node.end())Throw("missing metadata !"+std::to_string(id));
        uint32_t got;sv body;MetadataNode(lines[it->second],got,body);return body;
    }
    uint32_t Named(sv prefix)const {   // "!dx.x = !{!N}"
        for(const auto l:lines)if(StartsWith(l,prefix)) {
            const auto b=l.find("!{");const auto f=Fields(l.substr(b+2,l.size()-b-3));
            if(f.size()!=1)Throw("named metadata with several nodes");
            return NodeRef(f[0]);
        }
        Throw("missing "+std::string(prefix));
    }
    // which: 0 inputs, 1 outputs of the (single) entry point; list: the list node (0 when null)
    std::vector<Element> Signature(int which,uint32_t& list)const {
        const auto entry=Fields(Body(Named("!dx.entryPoints = ")));
        if(entry.size()!=5 || entry[2]=="null"){list=0;return {};}
        const auto sigs=Fields(Body(NodeRef(entry[2])));
        if(sigs.size()!=3 || sigs[which]=="null"){list=0;return {};}
        list=NodeRef(sigs[which]);
        std::vector<Element> out;
        for(const auto m:Fields(Body(list))) {
            Element e;e.node=NodeRef(m);const auto f=Fields(Body(e.node));
            if(f.size()!=11)Throw("unexpected signature element");
            for(size_t i=0;i<11;++i)e.fields[i]=std::string(f[i]);
            e.id=Number(f[0].substr(4));e.name=std::string(f[1]);e.kind=Number(f[3].substr(3));
            e.indices=f[4]=="null"?std::string():std::string(Body(NodeRef(f[4])));
            e.rows=Number(f[6].substr(4));e.row=uint32_t(int32_t(Number64(f[8].substr(4))));e.col=Number(f[9].substr(3));
            for(auto& c:e.name)c=char(std::toupper(static_cast<unsigned char>(c)));
            out.push_back(std::move(e));
        }
        return out;
    }
};
// PS inputs the system makes (front face, primitive id, sample index): not linked to the vertex shader.
bool SystemInput(uint32_t kind){return kind==10 || kind==12 || kind==13;}
}

IrTwin RewriteForViews(const std::string& text,const std::string* vertex) {
    IrTwin out;
    try {
        std::vector<sv> lines;
        {sv all(text);size_t start=0;
         for(size_t i=0;i<all.size();++i)if(all[i]=='\n'){lines.push_back(all.substr(start,i-start));start=i+1;}
         if(start<all.size())lines.push_back(all.substr(start));}
        for(auto& l:lines)if(!l.empty() && l.back()=='\r')l.remove_suffix(1);
        std::unordered_map<uint32_t,size_t> node;   // metadata id -> line
        uint32_t maxNode=0;size_t define=SIZE_MAX,resourcesLine=SIZE_MAX,viewIdLine=SIZE_MAX,smLine=SIZE_MAX,versionLine=SIZE_MAX,
               entryLine=SIZE_MAX;
        for(size_t i=0;i<lines.size();++i) {
            const sv l=lines[i];uint32_t id;sv body;
            if(MetadataNode(l,id,body)){node[id]=i;maxNode=std::max(maxNode,id);continue;}
            if(l.size()>1 && l[0]=='!' && std::isdigit(static_cast<unsigned char>(l[1]))) {   // "!52 = distinct !{...}"
                const auto eq=l.find(" = ");
                if(eq!=sv::npos){maxNode=std::max(maxNode,Number(l.substr(1,eq-1)));continue;}
            }
            if(StartsWith(l,"define ")){if(define!=SIZE_MAX)Throw("more than one function");define=i;}
            else if(StartsWith(l,"!dx.resources = "))resourcesLine=i;
            else if(StartsWith(l,"!dx.viewIdState = "))viewIdLine=i;
            else if(StartsWith(l,"!dx.shaderModel = "))smLine=i;
            else if(StartsWith(l,"!dx.version = "))versionLine=i;
            else if(StartsWith(l,"!dx.entryPoints = "))entryLine=i;
        }
        if(define==SIZE_MAX || smLine==SIZE_MAX || versionLine==SIZE_MAX || entryLine==SIZE_MAX)Throw("missing module structure");
        if(text.find("@dx.op.viewID.")!=std::string::npos)Throw("already reads SV_ViewID");
        const auto nodeBody=[&](uint32_t id)->sv {
            const auto it=node.find(id);if(it==node.end())Throw("missing metadata !"+std::to_string(id));
            uint32_t got;sv body;MetadataNode(lines[it->second],got,body);return body;
        };
        const auto namedRef=[&](size_t line)->uint32_t {   // "!dx.x = !{!N}"
            const sv l=lines[line];const auto b=l.find("!{");
            if(b==sv::npos || l.back()!='}')Throw("unexpected named metadata");
            const auto f=Fields(l.substr(b+2,l.size()-b-3));
            if(f.size()!=1)Throw("named metadata with several nodes");
            return NodeRef(f[0]);
        };
        // ---- shader kind and model
        const auto sm=Fields(nodeBody(namedRef(smLine)));
        if(sm.size()!=3 || (sm[0]!="!\"vs\"" && sm[0]!="!\"ps\""))Throw("not a vertex or pixel shader");
        const uint32_t smMajor=Number(sm[1].substr(4)),smMinor=Number(sm[2].substr(4));
        if(smMajor!=6)Throw("shader model not 6.x");
        // ---- the CBVs at b0..b3 space 0
        std::vector<Cbv> cbvs;uint32_t cbvList=0;uint32_t maxId=0;bool anyCbv=false;
        if(resourcesLine!=SIZE_MAX) {
            const auto lists=Fields(nodeBody(namedRef(resourcesLine)));
            if(lists.size()!=4)Throw("unexpected resource list");
            if(lists[2]!="null") {
                anyCbv=true;cbvList=NodeRef(lists[2]);
                for(const auto m:Fields(nodeBody(cbvList))) {
                    const uint32_t mid=NodeRef(m);const auto f=Fields(nodeBody(mid));
                    if(f.size()!=8)Throw("unexpected CBV record");
                    Cbv c{mid,Number(f[0].substr(4)),Number(f[3].substr(4)),Number(f[4].substr(4)),Number(f[5].substr(4)),
                          Number(f[6].substr(4)),std::string(f[1]),std::string(f[7])};
                    maxId=std::max(maxId,c.id);
                    if(c.space==0 && c.reg<=3) {
                        if(c.count!=1 || c.reg+c.count>4)Throw("b0..b3 as an array");
                        cbvs.push_back(c);
                    }else if(c.space==kTwinSpace)Throw("space 10 already used");
                }
            }
        }
        (void)anyCbv;
        // ---- a pixel shader that reads a system input (front face, ...) must, with view instancing, declare every output
        // of its vertex shader as an input, the elements in row order and the system inputs after them (else the
        // runtime refuses the pipeline; measured with the engine's SV_ClipDistance and SV_IsFrontFace).
        std::vector<Element> psInputs,added;uint32_t inputList=0;
        std::unordered_map<uint32_t,uint32_t> newInputId;       // old input element id -> new
        std::unordered_map<uint32_t,std::string> elementLine;   // input element node -> its new line
        std::vector<uint32_t> newOrder;                          // input list after the rewrite (nodes)
        std::vector<std::string> extraNodes;                     // added element and index nodes
        Module self(text);
        uint32_t nextNode=self.maxNode+64;   // above the nodes the writer below allocates
        if(sm[0]=="!\"ps\"" && vertex) {
            psInputs=self.Signature(0,inputList);
            const bool system=std::any_of(psInputs.begin(),psInputs.end(),[](const Element& e){return SystemInput(e.kind);});
            if(system) {
                Module vs(*vertex);uint32_t vsList=0;
                const auto vsOutputs=vs.Signature(1,vsList);
                for(const auto& o:vsOutputs) {
                    const bool linked=std::any_of(psInputs.begin(),psInputs.end(),[&](const Element& e){
                        return e.name==o.name && e.indices==o.indices;});
                    if(!linked)added.push_back(o);
                }
                // the order: linked elements and the added ones by row/column, then the system inputs on new rows
                struct Item {const Element* e;bool add;};
                std::vector<Item> order;
                for(const auto& e:psInputs)if(!SystemInput(e.kind))order.push_back({&e,false});
                for(const auto& e:added)order.push_back({&e,true});
                std::stable_sort(order.begin(),order.end(),[](const Item& a,const Item& b){
                    return a.e->row!=b.e->row?a.e->row<b.e->row:a.e->col<b.e->col;});
                uint32_t end=0;for(const auto& it:order)if(int32_t(it.e->row)>=0)end=std::max(end,it.e->row+it.e->rows);
                for(const auto& e:psInputs)if(SystemInput(e.kind))order.push_back({&e,false});
                bool moved=!added.empty();
                for(size_t i=0;i<order.size();++i)if(!order[i].add && order[i].e->id!=i)moved=true;
                for(const auto& e:psInputs)if(SystemInput(e.kind) && e.row<end)moved=true;
                if(moved) {
                    uint32_t systemRow=end;
                    for(size_t i=0;i<order.size();++i) {
                        const Element& e=*order[i].e;
                        std::string f[11];for(size_t k=0;k<11;++k)f[k]=e.fields[k];
                        f[0]="i32 "+std::to_string(i);
                        if(SystemInput(e.kind)){f[8]="i32 "+std::to_string(systemRow);f[9]="i8 0";systemRow+=e.rows;}
                        uint32_t nodeId=e.node;
                        if(order[i].add) {
                            nodeId=nextNode++;
                            if(e.indices.empty())f[4]="null";
                            else{const uint32_t idx=nextNode++;extraNodes.push_back("!"+std::to_string(idx)+" = !{"+e.indices+"}");f[4]="!"+std::to_string(idx);}
                            f[10]="null";
                        }else newInputId[e.id]=uint32_t(i);
                        std::string b;for(size_t k=0;k<11;++k)b+=(k?", ":"")+f[k];
                        const std::string record="!"+std::to_string(nodeId)+" = !{"+b+"}";
                        if(order[i].add)extraNodes.push_back(record);else elementLine[nodeId]=record;
                        newOrder.push_back(nodeId);
                    }
                    out.signature=true;
                }
            }
        }
        if(cbvs.empty() && !out.signature){out.ok=out.unchanged=true;return out;}
        std::unordered_map<uint32_t,uint32_t> twinId;   // original range id -> twin range id
        for(size_t i=0;i<cbvs.size();++i)twinId[cbvs[i].id]=maxId+1+uint32_t(i);
        // ---- handles, loads, extracts
        std::unordered_map<std::string,std::string> handleTwin;      // %h -> %cvr.hN
        struct Load {std::string twin;const char* element;};
        std::unordered_map<std::string,Load> loadTwin;              // %v -> twin load
        std::vector<int> kind(lines.size(),0);                      // 1 handle def, 2 load def, 3 extract
        for(size_t i=define;i<lines.size();++i) {
            sv name,rest;
            if(!Definition(lines[i],name,rest))continue;
            const sv code=CodePart(rest);
            static constexpr sv kHandle="call %dx.types.Handle @dx.op.createHandle(i32 57, i8 2, i32 ";
            if(StartsWith(code,kHandle)) {
                const auto f=Fields(code.substr(kHandle.size()-4,code.size()-(kHandle.size()-4)-1));   // "i32 ID, i32 IDX, i1 NU"
                if(f.size()!=3)Throw("unexpected createHandle");
                const uint32_t id=Number(f[0].substr(4));
                if(twinId.count(id)) {
                    handleTwin[std::string(name)]="%cvr.h"+std::to_string(out.handles++);
                    kind[i]=1;
                }
                continue;
            }
            static constexpr sv kLoad="call %dx.types.CBufRet.";
            if(StartsWith(code,kLoad)) {
                const auto at=code.find(" @dx.op.cbufferLoadLegacy.");
                if(at==sv::npos)continue;
                const sv ret=code.substr(5,at-5);
                const auto open=code.find('(');
                const auto f=Fields(code.substr(open+1,code.size()-open-2));
                if(f.size()!=3 || f[0]!="i32 59" || !StartsWith(f[1],"%dx.types.Handle "))Throw("unexpected cbufferLoadLegacy");
                const std::string h(f[1].substr(17));
                if(!handleTwin.count(h))continue;
                const char* element=ElementType(ret);
                if(!element)Throw("unknown CBufRet type");
                loadTwin[std::string(name)]={"%cvr.l"+std::to_string(out.loads++),element};
                kind[i]=2;
                continue;
            }
            if(StartsWith(code,"extractvalue %dx.types.CBufRet.")) {
                const auto f=Fields(code.substr(13));
                if(f.size()!=2)Throw("unexpected extractvalue");
                const auto sp=f[0].rfind(' ');
                if(sp==sv::npos)Throw("unexpected extractvalue operand");
                if(loadTwin.count(std::string(f[0].substr(sp+1)))){kind[i]=3;++out.extracts;}
            }
        }
        // ---- every other use of a twinned handle or load is outside the recognised shape
        for(size_t i=define;i<lines.size();++i) {
            sv name,rest;const bool def=Definition(lines[i],name,rest);
            const sv code=def?CodePart(rest):CodePart(lines[i]);
            Tokens(code,[&](sv tok) {
                const std::string t(tok);
                if(handleTwin.count(t) && !(kind[i]==2))Throw("b0..b3 handle used outside cbufferLoadLegacy");
                if(loadTwin.count(t) && kind[i]!=3)Throw("constant-buffer load used outside extractvalue");
            });
        }
        // ---- write the module
        std::string w;w.reserve(text.size()+out.loads*220+out.extracts*160+4096);
        const auto line=[&](sv s){w.append(s.data(),s.size());w+='\n';};
        const auto replaceAll=[](std::string s,const std::string& from,const std::string& to) {
            for(size_t p=0;(p=s.find(from,p))!=std::string::npos;) {
                const size_t e=p+from.size();
                if(e<s.size() && IsNameChar(s[e])){p=e;continue;}   // %12 inside %123
                s.replace(p,from.size(),to);p+=to.size();
            }
            return s;
        };
        uint32_t newNode=maxNode+1;
        const uint32_t smNode=newNode++,versionNode=newNode++,propsNode=newNode++;
        std::vector<uint32_t> twinNodes;for(size_t i=0;i<cbvs.size();++i)twinNodes.push_back(newNode++);
        uint32_t extractSerial=0;
        // entry point properties: ShaderFlags (tag 0) gains ViewID (bit 21)
        const uint32_t entryNode=namedRef(entryLine);
        std::string props;
        {
            const auto f=Fields(nodeBody(entryNode));
            if(f.size()!=5)Throw("unexpected entry point record");
            std::string body;bool tagged=false;
            if(f[4]!="null") {
                const auto p=Fields(nodeBody(NodeRef(f[4])));
                if(p.size()%2)Throw("odd entry properties");
                for(size_t i=0;i<p.size();i+=2) {
                    std::string value(p[i+1]);
                    if(p[i]=="i32 0"){
                        if(!StartsWith(p[i+1],"i64 "))Throw("unexpected shader flags");
                        value="i64 "+std::to_string(Number64(p[i+1].substr(4))|(1ull<<21));tagged=true;
                    }
                    body+=(body.empty()?"":", ")+std::string(p[i])+", "+value;
                }
            }
            if(!tagged)body=std::string("i32 0, i64 ")+std::to_string(1ull<<21)+(body.empty()?"":", "+body);
            props="!"+std::to_string(propsNode)+" = !{"+body+"}";
        }
        for(size_t i=0;i<lines.size();++i) {
            const sv l=lines[i];
            if(i==viewIdLine)continue;   // regenerated by -viewid-state
            if(i==smLine){line("!dx.shaderModel = !{!"+std::to_string(smNode)+"}");continue;}
            if(i==versionLine){line("!dx.version = !{!"+std::to_string(versionNode)+"}");continue;}
            uint32_t id;sv body;
            if(MetadataNode(l,id,body)) {
                if(id==cbvList) {
                    std::string b(body);for(const auto n:twinNodes)b+=", !"+std::to_string(n);
                    line("!"+std::to_string(id)+" = !{"+b+"}");continue;
                }
                if(id==entryNode) {
                    auto f=Fields(body);std::string b;
                    for(size_t k=0;k<4;++k)b+=std::string(f[k])+", ";
                    line("!"+std::to_string(id)+" = !{"+b+"!"+std::to_string(propsNode)+"}");continue;
                }
                if(out.signature && id==inputList) {
                    std::string b;for(size_t k=0;k<newOrder.size();++k)b+=(k?", !":"!")+std::to_string(newOrder[k]);
                    line("!"+std::to_string(id)+" = !{"+b+"}");continue;
                }
                if(out.signature)if(const auto it=elementLine.find(id);it!=elementLine.end()){line(it->second);continue;}
                line(l);continue;
            }
            if(i==define) {
                line(l);
                line("  %cvr.view = call i32 @dx.op.viewID.i32(i32 138)  ; ViewID()");
                line("  %cvr.left = icmp eq i32 %cvr.view, 0");
                continue;
            }
            if(out.signature && i>define && StartsWith(l,"  ")) {   // loadInput (4), evalSnapped (87), evalSampleIndex (88), evalCentroid (89), attributeAtVertex (137)
                static const char* const ops[]{"@dx.op.loadInput.","@dx.op.evalSnapped.","@dx.op.evalSampleIndex.","@dx.op.evalCentroid.",
                                               "@dx.op.attributeAtVertex."};
                bool rewritten=false;
                for(const char* op:ops) {
                    const auto at=l.find(op);if(at==sv::npos)continue;
                    const auto open=l.find('(',at);const auto comma1=open==sv::npos?sv::npos:l.find(", i32 ",open);
                    if(comma1==sv::npos)Throw("unexpected input read");
                    const auto idStart=comma1+6;auto idEnd=idStart;while(idEnd<l.size() && std::isdigit(static_cast<unsigned char>(l[idEnd])))++idEnd;
                    if(idEnd==idStart)Throw("dynamic input element id");
                    const auto it=newInputId.find(Number(l.substr(idStart,idEnd-idStart)));
                    if(it==newInputId.end())Throw("input element without a new id");
                    line(std::string(l.substr(0,idStart))+std::to_string(it->second)+std::string(l.substr(idEnd)));
                    rewritten=true;break;
                }
                if(rewritten)continue;
            }
            sv name,rest;
            if(kind[i]==1 && Definition(l,name,rest)) {
                line(l);
                const sv code=CodePart(rest);
                static constexpr sv kHandle="call %dx.types.Handle @dx.op.createHandle(i32 57, i8 2, i32 ";
                const auto f=Fields(code.substr(kHandle.size()-4,code.size()-(kHandle.size()-4)-1));
                line("  "+handleTwin.at(std::string(name))+" = call %dx.types.Handle @dx.op.createHandle(i32 57, i8 2, i32 "+
                     std::to_string(twinId.at(Number(f[0].substr(4))))+", "+std::string(f[1])+", "+std::string(f[2])+")");
                continue;
            }
            if(kind[i]==2 && Definition(l,name,rest)) {
                line(l);
                const sv code=CodePart(rest);
                const auto open=code.find('(');
                const auto f=Fields(code.substr(open+1,code.size()-open-2));
                const std::string h(f[1].substr(17));
                line("  "+loadTwin.at(std::string(name)).twin+" = "+std::string(code.substr(0,open+1))+"i32 59, %dx.types.Handle "+
                     handleTwin.at(h)+", "+std::string(f[2])+")");
                continue;
            }
            if(kind[i]==3 && Definition(l,name,rest)) {
                const sv code=CodePart(rest);
                const auto f=Fields(code.substr(13));
                const auto sp=f[0].rfind(' ');
                const std::string v(f[0].substr(sp+1));
                const auto& load=loadTwin.at(v);
                const std::string serial=std::to_string(extractSerial++);
                const std::string own="%cvr.r"+serial,other="%cvr.e"+serial;
                line("  "+own+" = "+std::string(code));
                line("  "+other+" = "+replaceAll(std::string(code),v,load.twin));
                line("  "+std::string(name)+" = select i1 %cvr.left, "+load.element+" "+other+", "+load.element+" "+own);
                continue;
            }
            line(l);
        }
        line("declare i32 @dx.op.viewID.i32(i32) nounwind readnone");
        {
            std::string kindName(sm[0]);
            line("!"+std::to_string(smNode)+" = !{"+kindName+", i32 6, i32 "+std::to_string(std::max(smMinor,1u))+"}");
            const auto v=Fields(nodeBody(namedRef(versionLine)));
            if(v.size()!=2)Throw("unexpected DXIL version");
            line("!"+std::to_string(versionNode)+" = !{i32 1, i32 "+std::to_string(std::max(Number(v[1].substr(4)),1u))+"}");
            line(props);
            for(const auto& n:extraNodes)line(n);
            for(size_t i=0;i<cbvs.size();++i) {
                const auto& c=cbvs[i];
                line("!"+std::to_string(twinNodes[i])+" = !{i32 "+std::to_string(twinId.at(c.id))+", "+c.type+", !\"\", i32 "+
                     std::to_string(kTwinSpace)+", i32 "+std::to_string(c.reg)+", i32 1, i32 "+std::to_string(c.size)+", "+c.ext+"}");
            }
        }
        out.text=std::move(w);
        out.ok=true;
    } catch(const Fail& f) {
        out=IrTwin{};out.error=f.why;
    } catch(const std::exception& e) {
        out=IrTwin{};out.error=e.what();
    }
    return out;
}
}
