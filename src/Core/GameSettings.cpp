#include "Core/GameSettings.hpp"
#include <windows.h>
#include <json/json.h>
#include <fstream>
#include <memory>
#include <set>
#include <unordered_map>

namespace cvr::settings {
namespace {
bool InGroup(std::string_view group,std::string_view root) {
    return group==root || (group.starts_with(root) && group.size()>root.size() && group[root.size()]=='/');
}
// The player's own: language, sound, difficulty, gallery, telemetry and cloud saves, and the picture
// calibration -- brightness, gamma and the HDR brightness levels are set for the player's eyes and
// display, so the VR preset never carries or overwrites them (the HDR mode itself stays in it).
bool Preserve(std::string_view group,std::string_view name) {
    return InGroup(group,"/language") || InGroup(group,"/audio") ||
        InGroup(group,"/gameplay/difficulty") || InGroup(group,"/gallery") || InGroup(group,"/brightness") ||
        (group=="/gameplay/misc" && (name=="EnableTelemetry" || name=="EnableCloudSaves")) ||
        (group=="/video/display" && (name=="Gamma" || name=="PaperWhiteLevel" ||
                                     name=="MaxMonitorBrightness" || name=="TonemappingMidpoint"));
}
bool Parse(std::string_view text,Json::Value& root,std::string& error) {
    Json::CharReaderBuilder builder;Json::CharReaderBuilder::strictMode(&builder.settings_);
    builder["collectComments"]=false;
    std::unique_ptr<Json::CharReader> reader(builder.newCharReader());
    if(!reader->parse(text.data(),text.data()+text.size(),&root,&error))return false;
    if(!root.isObject() || !root["data"].isArray()){error="Expected a settings object with a data array";return false;}
    std::set<std::string> groups;
    for(const auto& group:root["data"]) {
        if(!group.isObject() || !group["group_name"].isString() || group["group_name"].asString().empty() ||
           !group["options"].isArray()){error="Invalid settings group";return false;}
        const auto name=group["group_name"].asString();
        if(!groups.insert(name).second){error="Duplicate settings group: "+name;return false;}
        std::set<std::string> options;
        for(const auto& option:group["options"]) {
            if(!option.isObject() || !option["name"].isString() || option["name"].asString().empty()) {
                error="Invalid setting in "+name;return false;
            }
            // A button is an action in the menu, not a value: 2.5 writes /gameplay/misc TakeoutTelemetry
            // ("type": "button") with no "value" at all, and every player file carries it.
            const bool button=option["type"].isString() && option["type"].asString()=="button";
            if(!button && !option.isMember("value")){error="Invalid setting in "+name;return false;}
            if(!options.insert(option["name"].asString()).second){error="Duplicate setting in "+name;return false;}
        }
    }
    return true;
}
bool Read(const std::filesystem::path& path,std::string& text) {
    std::ifstream file(path,std::ios::binary|std::ios::ate);
    if(!file)return false;
    const auto size=file.tellg();
    if(size<=0 || size>16*1024*1024)return false;
    text.resize(static_cast<size_t>(size));file.seekg(0);
    return static_cast<bool>(file.read(text.data(),static_cast<std::streamsize>(size)));
}
std::string WinError(const char* action) {return std::string(action)+" (Windows error "+std::to_string(GetLastError())+")";}
// The settings file replaced in one step: written beside it under a temporary name, flushed, then moved
// over it, so the game never reads half a file. `backup`, when given, receives a copy of the old one first.
bool WriteAtomically(const std::filesystem::path& player,const std::string& text,const wchar_t* suffix,
                     const std::filesystem::path* backup,std::string& error) {
    const auto temporary=player.parent_path()/(L"UserSettings.vrport-"+std::wstring(suffix)+L".tmp");
    HANDLE file=CreateFileW(temporary.c_str(),GENERIC_WRITE,0,nullptr,CREATE_NEW,FILE_ATTRIBUTE_NORMAL,nullptr);
    if(file==INVALID_HANDLE_VALUE){error=WinError("Cannot create temporary settings");return false;}
    DWORD written{};
    const bool saved=WriteFile(file,text.data(),static_cast<DWORD>(text.size()),&written,nullptr) &&
        written==text.size() && FlushFileBuffers(file);
    if(!saved)error=WinError("Cannot write temporary settings");
    CloseHandle(file);
    bool ok=false;
    if(saved) {
        if(backup && !CopyFileW(player.c_str(),backup->c_str(),TRUE))error=WinError("Cannot back up player settings");
        else if(MoveFileExW(temporary.c_str(),player.c_str(),MOVEFILE_REPLACE_EXISTING|MOVEFILE_WRITE_THROUGH))ok=true;
        else error=WinError("Cannot install settings");
    }
    if(!ok)DeleteFileW(temporary.c_str());
    return ok;
}
// THE PRESET'S VALUE GOES IN ONLY WHERE THE PLAYER'S FILE ACCEPTS IT. That file is the game's own schema for the
// build that wrote it, and one value the game cannot read makes it throw the whole file away ("the user settings
// file is corrupted", everything back to defaults -- seen on a clean 2.5 install from the 2.31 preset). So:
//  - a list value must be one of the player's list: by name, or by position where the list kept its length and
//    only its labels changed (2.31's InputSchemesMode values were localisation keys, 2.5's are names);
//  - a dynamic list is filled from the hardware at run time: written only where the player's own entry holds a
//    choice (an empty one -- DLSS_MultiFrameGeneration on a card without it -- is a list with nothing in it);
//  - a number must lie in the player's range (2.5's FPP_PadZoomFactor tops out at 1, 2.31's at 4);
//  - anything else keeps its JSON kind.
// `value` and `index` receive what to write; false leaves the player's entry as it is.
bool SameKind(const Json::Value& a,const Json::Value& b) {return a.type()==b.type() || (a.isNumeric() && b.isNumeric());}
bool Accepts(const Json::Value& target,const Json::Value& source,Json::Value& value,Json::Value& index) {
    const auto type=target["type"].asString();
    if(type=="button" || (source["type"].isString() && source["type"].asString()!=type))return false;
    const auto& wanted=source["value"];
    if(type.ends_with("_list")) {
        if(target["is_dynamic"].asBool()) {
            const bool chosen=target["index"].isInt() && target["index"].asInt()>=0 &&
                !(target["value"].isString() && target["value"].asString().empty());
            if(!chosen || !SameKind(wanted,target["value"]))return false;
            value=wanted;
            if(source.isMember("index"))index=source["index"];
            return true;
        }
        const auto& values=target["values"];
        if(!values.isArray() || values.empty())return false;
        for(Json::ArrayIndex i=0;i<values.size();++i)
            if(values[i]==wanted){value=wanted;index=static_cast<int>(i);return true;}
        const auto& theirs=source["values"];
        const int at=source["index"].isInt()?source["index"].asInt():-1;
        if(!theirs.isArray() || theirs.size()!=values.size() || at<0 || at>=static_cast<int>(values.size()))return false;
        value=values[static_cast<Json::ArrayIndex>(at)];index=at;
        return true;
    }
    if(type=="int" || type=="float") {
        if(!wanted.isNumeric() || (type=="int" && !wanted.isIntegral()))return false;
        const double v=wanted.asDouble();
        if((target["min_value"].isNumeric() && v<target["min_value"].asDouble()) ||
           (target["max_value"].isNumeric() && v>target["max_value"].asDouble()))return false;
        value=type=="int"?Json::Value(wanted.asInt()):wanted;
        return true;
    }
    if(!SameKind(wanted,target["value"]))return false;
    value=wanted;
    return true;
}
std::wstring Suffix() {
    SYSTEMTIME time{};GetLocalTime(&time);
    wchar_t suffix[96]{};
    swprintf_s(suffix,L"%04u%02u%02u-%02u%02u%02u-%03u-%lu",time.wYear,time.wMonth,time.wDay,
               time.wHour,time.wMinute,time.wSecond,time.wMilliseconds,GetCurrentProcessId());
    return suffix;
}
}
MergeResult MergeGameSettings(std::string_view player,std::string_view preset) {
    MergeResult result;Json::Value current,shipped;
    if(!Parse(player,current,result.error)){result.error="Player settings: "+result.error;return result;}
    if(!Parse(preset,shipped,result.error)){result.error="VR preset: "+result.error;return result;}
    auto& groups=current["data"];
    std::unordered_map<std::string,Json::ArrayIndex> byGroup;
    for(Json::ArrayIndex i=0;i<groups.size();++i)byGroup.emplace(groups[i]["group_name"].asString(),i);
    for(const auto& sourceGroup:shipped["data"]) {
        const auto groupName=sourceGroup["group_name"].asString();
        if(Preserve(groupName,{}))continue;
        // A group or an option the player's file does not have is one this build of the game does not have:
        // nothing is added (2.5 has no /controls SteeringSensitivity, which 2.31's preset carries).
        const auto found=byGroup.find(groupName);
        if(found==byGroup.end()) {
            for(const auto& option:sourceGroup["options"])
                if(!Preserve(groupName,option["name"].asString()))result.skipped.push_back(groupName+"/"+option["name"].asString());
            continue;
        }
        auto& options=groups[found->second]["options"];
        std::unordered_map<std::string,Json::ArrayIndex> byName;
        for(Json::ArrayIndex i=0;i<options.size();++i)byName.emplace(options[i]["name"].asString(),i);
        for(const auto& source:sourceGroup["options"]) {
            const auto name=source["name"].asString();
            if(Preserve(groupName,name))continue;
            const auto option=byName.find(name);
            Json::Value value,index;
            if(option==byName.end() || !Accepts(options[option->second],source,value,index)) {
                result.skipped.push_back(groupName+"/"+name);
                continue;
            }
            // Keep the player's schema/defaults and unknown fields. Only the
            // saved value and its list index belong to the first-launch preset.
            auto& target=options[option->second];
            bool changed=target["value"]!=value;
            target["value"]=value;
            if(!index.isNull()) {
                changed|=target["index"]!=index;target["index"]=index;
            }
            if(changed)++result.changed;
        }
    }
    Json::StreamWriterBuilder writer;writer["indentation"]="    ";
    result.json=Json::writeString(writer,current)+"\n";result.ok=true;return result;
}
InstallResult InstallGameSettings(const std::filesystem::path& preset,const std::filesystem::path& player) {
    InstallResult result;std::string source,existing;
    if(!Read(preset,source)){result.error="Cannot read VR preset";return result;}
    if(!Read(player,existing)){result.error="Cannot read existing player settings";return result;}
    const auto merged=MergeGameSettings(existing,source);
    if(!merged.ok){result.error=merged.error;return result;}
    result.changed=merged.changed;result.skipped=merged.skipped;
    if(!merged.changed){result.ok=true;return result;}
    const auto suffix=Suffix();
    const auto backup=player.parent_path()/(L"UserSettings.pre-vr-"+suffix+L".json");
    result.ok=WriteAtomically(player,merged.json,suffix.c_str(),&backup,result.error);
    if(result.ok)result.backup=backup;
    return result;
}
SetResult SetGameSetting(const std::filesystem::path& player,std::string_view group,std::string_view name,
                         std::string_view value,int index) {
    SetResult result;std::string text;Json::Value root;
    if(!Read(player,text)){result.error="Cannot read player settings";return result;}
    if(!Parse(text,root,result.error)){result.error="Player settings: "+result.error;return result;}
    for(auto& g:root["data"]) {
        if(g["group_name"].asString()!=group)continue;
        for(auto& option:g["options"]) {
            if(option["name"].asString()!=name)continue;
            result.previous=option["value"].asString();
            if(result.previous==value && option["index"].asInt()==index){result.ok=true;return result;}
            option["value"]=std::string(value);option["index"]=index;
            Json::StreamWriterBuilder writer;writer["indentation"]="    ";
            result.ok=WriteAtomically(player,Json::writeString(writer,root)+"\n",Suffix().c_str(),nullptr,result.error);
            result.changed=result.ok;
            return result;
        }
    }
    result.error="No "+std::string(group)+"/"+std::string(name)+" in the player settings";
    return result;
}
std::filesystem::path PlayerSettingsFile() {
    wchar_t local[MAX_PATH]={};
    const auto length=GetEnvironmentVariableW(L"LOCALAPPDATA",local,MAX_PATH);
    if(!length || length>=MAX_PATH)return {};
    return std::filesystem::path(local)/L"CD Projekt Red"/L"Cyberpunk 2077"/L"UserSettings.json";
}
std::string GetGameSetting(const std::filesystem::path& player,std::string_view group,std::string_view name) {
    std::string text,error;Json::Value root;
    if(!Read(player,text) || !Parse(text,root,error))return {};
    for(const auto& g:root["data"]) {
        if(g["group_name"].asString()!=group)continue;
        for(const auto& option:g["options"])
            if(option["name"].asString()==name && option["value"].isConvertibleTo(Json::stringValue))
                return option["value"].asString();
    }
    return {};
}
}
