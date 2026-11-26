"""Generate a C++ regression around the actual SDK call expression (not a copied substitute)."""
from pathlib import Path
import sys
root=Path(__file__).resolve().parents[2]
source=(root/'src/Anim/OpticHinge.cpp').read_text(encoding='utf-8')
body=source.split('bool IsActiveWeapon(',1)[1].split('\nbool Request(',1)[0]
start=body.index('ExecuteFunction(');at=start+len('ExecuteFunction');depth=0
for end in range(at,len(body)):
    if body[end]=='(':depth+=1
    elif body[end]==')':
        depth-=1
        if depth==0:break
call=body[start:end+1]
prefix=r'''
#include <cstdlib>
#include <iostream>
#include <vector>
struct CClass{};struct Function{};struct Entity{};
template<class T>struct Handle{T* instance;};
struct Weak{Entity* instance{};};struct Arg{void* value;};
Entity playerObject,weaponObject;int systems{},actors{};
bool ExecuteFunction(CClass*,Function*,Weak*,std::vector<Arg>&){++systems;return false;}
bool ExecuteFunction(void* context,Function*,Weak* result,std::vector<Arg>& args){
 ++actors;if(context!=&playerObject||args.size()!=1||static_cast<Handle<Entity>*>(args[0].value)->instance!=&playerObject)std::abort();
 result->instance=&weaponObject;return true;
}
int main(){CClass type;CClass* cls=&type;Function function;Function* fn=&function;
 Handle<Entity> player{&playerObject};Weak current;std::vector<Arg> args{{&player}};
 const bool result=
'''
suffix=r''';
 if(!result||systems||actors!=1||current.instance!=&weaponObject)return 1;
 std::cout<<"PASS production static-query call chooses actor void* context, preserves player argument, never GetSystem overload\n";
}
'''
Path(sys.argv[1]).write_text(prefix+call+suffix,encoding='utf-8')
