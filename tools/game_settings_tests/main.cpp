#include "Core/GameSettings.hpp"
#include <windows.h>
#include <json/json.h>
#include <fstream>
#include <iostream>
#include <memory>
#include <stdexcept>

namespace fs=std::filesystem;
void Check(bool ok,const char* why){if(!ok)throw std::runtime_error(why);}
Json::Value Parse(const std::string& text){
    Json::CharReaderBuilder builder;Json::Value value;std::string error;
    std::unique_ptr<Json::CharReader> reader(builder.newCharReader());
    Check(reader->parse(text.data(),text.data()+text.size(),&value,&error),"test JSON failed to parse");return value;
}
std::string Read(const fs::path& path){std::ifstream f(path,std::ios::binary);return {std::istreambuf_iterator<char>(f),{}};}
void Write(const fs::path& path,const std::string& text){std::ofstream f(path,std::ios::binary);f<<text;Check(bool(f),"fixture write failed");}
std::string Encode(const Json::Value& value){Json::StreamWriterBuilder writer;return Json::writeString(writer,value);}
Json::Value& Group(Json::Value& root,const std::string& name){
    for(auto& g:root["data"])if(g["group_name"]==name)return g;
    Json::Value g;g["group_name"]=name;g["options"]=Json::Value(Json::arrayValue);
    return root["data"].append(g);
}
Json::Value& Option(Json::Value& root,const std::string& group,const std::string& name){
    auto& options=Group(root,group)["options"];
    for(auto& option:options)if(option["name"]==name)return option;
    Json::Value option;option["name"]=name;option["value"]=0;return options.append(option);
}
bool Personal(const std::string& group,const std::string& name){
    return group=="/language" || group.starts_with("/audio/") || group=="/gameplay/difficulty" ||
        group.starts_with("/gallery/") || group=="/brightness" ||
        (group=="/gameplay/misc" && (name=="EnableTelemetry" || name=="EnableCloudSaves")) ||
        (group=="/video/display" && (name=="Gamma" || name=="PaperWhiteLevel" ||
                                     name=="MaxMonitorBrightness" || name=="TonemappingMidpoint"));
}
// The shipped preset over a real player file (argv[3], optional): every value it ends with is one that file's
// schema allows -- a fixed list's value among its values at its index, a number in range, nothing added.
void CheckRealFile(const fs::path& path,const Json::Value& preset){
    const auto text=Read(path);
    Check(!text.empty(),"real player file unreadable");
    const auto before=Parse(text);
    const auto merged=cvr::settings::MergeGameSettings(text,Encode(preset));
    Check(merged.ok,"the shipped preset over the real player file failed");
    const auto after=Parse(merged.json);
    Check(after["data"].size()==before["data"].size(),"the real file's group count changed");
    for(Json::ArrayIndex g=0;g<after["data"].size();++g){
        const auto& options=after["data"][g]["options"];
        Check(options.size()==before["data"][g]["options"].size(),"the real file's option count changed");
        for(Json::ArrayIndex i=0;i<options.size();++i){
            const auto& o=options[i];const auto& was=before["data"][g]["options"][i];
            const auto type=o["type"].asString();
            Check(o["name"]==was["name"] && o["type"]==was["type"],"the real file's schema changed");
            if(type.ends_with("_list") && !o["is_dynamic"].asBool()){
                const auto& values=o["values"];
                Check(o["index"].isInt() && o["index"].asInt()>=0 && o["index"].asUInt()<values.size() &&
                      values[o["index"].asUInt()]==o["value"],"a fixed list ended outside its values");
            } else if(type.ends_with("_list") && was["index"].asInt()<0)
                Check(o==was,"a dynamic list without a choice was given one");
            // Only what the merge wrote: the player's own file may already hold a number past its range
            // (2.5 writes MinMonitorBrightness 0.0002 under a 0.002 minimum and reads it back).
            else if((type=="int" || type=="float") && o!=was){
                const double v=o["value"].asDouble();
                Check((!o["min_value"].isNumeric() || v>=o["min_value"].asDouble()) &&
                      (!o["max_value"].isNumeric() || v<=o["max_value"].asDouble()),"a number ended outside its range");
            }
            if(o.isMember("value") && was.isMember("value"))
                Check(o["value"].type()==was["value"].type() || (o["value"].isNumeric() && was["value"].isNumeric()),
                      "a value changed its JSON kind");
        }
    }
    std::cout<<"real file "<<path.filename().string()<<": "<<merged.changed<<" changed, "<<merged.skipped.size()<<" skipped";
    for(const auto& s:merged.skipped)std::cout<<"\n  skipped "<<s;
    std::cout<<"\n";
}
int main(int argc,char** argv)try {
    Check(argc==3 || argc==4,"preset and output directory required (and optionally a real player file)");
    auto preset=Parse(Read(fs::path(argv[1]))),player=preset;
    bool reflexFound=false;
    for(const auto& group:preset["data"])for(const auto& option:group["options"])
        if(option["name"]=="ReflexMode") {
            reflexFound=true;
            Check(option["value"]=="Disabled" && option["index"]==0,"shipped Reflex preset is not Off");
        }
    Check(reflexFound,"shipped Reflex preset missing");
    bool crosshairFound=false;
    for(const auto& group:preset["data"])for(const auto& option:group["options"]) {
        const auto groupName=group["group_name"].asString(),name=option["name"].asString();
        Check(groupName!="/brightness" && !(groupName=="/video/display" && Personal(groupName,name)),
              "shipped preset carries a picture calibration value");
        if(groupName=="/interface/hud" && name=="crosshairs") {
            crosshairFound=true;Check(option["value"]==false,"shipped HUD crosshair is not off");
        }
    }
    Check(crosshairFound,"shipped HUD crosshair setting missing");
    player["version"]=999;player["player_metadata"]="keep";
    unsigned expected=0;
    for(auto& group:player["data"])for(auto& option:group["options"]){
        const auto name=option["name"].asString(),groupName=group["group_name"].asString();
        if(option["value"].isBool())option["value"]=!option["value"].asBool();
        else if(option["value"].isNumeric())option["value"]=option["value"].asDouble()+123;
        else if(option["value"].isString())option["value"]="player_"+option["value"].asString();
        else option["value"]=Json::Value(Json::arrayValue);
        if(option.isMember("index"))option["index"]=999;
        option["player_schema_field"]=31;
        if(!Personal(groupName,name))++expected;
    }
    for(const char* name:{"OnScreen","Platform","Subtitles","VoiceOver"})Option(player,"/language",name)["value"]="ru";
    Option(player,"/gameplay/difficulty","GameDifficulty")["value"]="Story";
    Option(player,"/gameplay/misc","EnableTelemetry")["value"]=false;
    Option(player,"/gameplay/misc","EnableCloudSaves")["value"]=false;
    Option(player,"/gallery/favorites","GalleryFavorites")["value"]="my photos";
    Option(player,"/controls","future_binding")["value"]="keep binding";
    Option(player,"/future/version","Keep")["value"]="unknown group";
    const auto before=player;
    const auto merged=cvr::settings::MergeGameSettings(Encode(player),Encode(preset));
    Check(merged.ok && merged.changed==expected,"not every intended preset value was copied");
    auto result=Parse(merged.json);
    for(auto& group:preset["data"])for(auto& option:group["options"]){
        const auto path=group["group_name"].asString(),name=option["name"].asString();
        const auto& actual=Option(result,path,name);
        if(Personal(path,name))Check(actual==Option(player,path,name),"personal setting was altered");
        else {
            Check(actual["value"]==option["value"],"preset setting was not applied");
            if(option.isMember("index"))Check(actual["index"]==option["index"],"list selection index was not applied");
            Check(actual["player_schema_field"]==31,"player schema was replaced");
        }
    }
    Check(Group(result,"/language")==Group(player,"/language"),"language group disappeared");
    Check(result["version"]==999 && result["player_metadata"]=="keep","player metadata replaced");
    Check(Option(result,"/controls","future_binding")["value"]=="keep binding","unknown input setting lost");
    Check(Group(result,"/future/version")==Group(player,"/future/version"),"unknown group lost");
    Check(cvr::settings::MergeGameSettings(merged.json,Encode(preset)).changed==0,"merge is not idempotent");
    {
        // A preset that does carry them still leaves the player's brightness, gamma and HDR levels alone.
        auto calibrated=preset,own=player;
        const std::pair<const char*,const char*> picture[]={{"/brightness","Brightness"},{"/video/display","Gamma"},
            {"/video/display","PaperWhiteLevel"},{"/video/display","MaxMonitorBrightness"},{"/video/display","TonemappingMidpoint"}};
        for(const auto& [group,name]:picture){Option(calibrated,group,name)["value"]=1.0;Option(own,group,name)["value"]=77.5;}
        const auto kept=cvr::settings::MergeGameSettings(Encode(own),Encode(calibrated));
        Check(kept.ok,"calibration merge failed");
        auto keptJson=Parse(kept.json);
        for(const auto& [group,name]:picture)Check(Option(keptJson,group,name)["value"]==77.5,"player's brightness or gamma was overwritten");
    }
    // 2.5 carries a menu button with no value (/gameplay/misc TakeoutTelemetry) in every player file.
    {
        auto withButton=player;
        Json::Value button;button["name"]="TakeoutTelemetry";button["type"]="button";button["is_dynamic"]=false;
        button["button_type"]="takeout_telemetry";
        Group(withButton,"/gameplay/misc")["options"].append(button);
        const auto kept=cvr::settings::MergeGameSettings(Encode(withButton),Encode(preset));
        Check(kept.ok,"a player file with a menu button was rejected");
        auto keptJson=Parse(kept.json);bool found=false;
        for(const auto& o:Group(keptJson,"/gameplay/misc")["options"])
            if(o["name"]=="TakeoutTelemetry")found=!o.isMember("value") && o["type"]=="button";
        Check(found,"the menu button did not survive the merge");
    }
    // A file with nothing in it takes nothing: a group or an option the player's file lacks is not this game build's.
    const std::string empty=R"({"version":7,"data":[]})";
    const auto fresh=cvr::settings::MergeGameSettings(empty,Encode(preset));
    Check(fresh.ok && fresh.changed==0 && !fresh.skipped.empty(),"an empty player file was not left alone");
    Check(Parse(fresh.json)["data"].empty(),"preset groups were inserted into a player file without them");
    {
        // What a 2.31 preset meets in a 2.5 file (seen on a clean install: the game threw the merged file away).
        Json::Value now,old;now["version"]=152;old["version"]=140;
        auto& scheme=Option(now,"/controls/controller","InputSchemesMode");
        scheme["type"]="string_list";scheme["is_dynamic"]=false;scheme["value"]="Agile";scheme["index"]=1;
        for(const char* v:{"Legacy","Agile","Alternative"})scheme["values"].append(v);
        auto& oldScheme=Option(old,"/controls/controller","InputSchemesMode");
        oldScheme["type"]="string_list";oldScheme["value"]="LocKey#3";oldScheme["index"]=2;
        for(const char* v:{"LocKey#1","LocKey#2","LocKey#3"})oldScheme["values"].append(v);
        auto& quality=Option(now,"/graphics/advanced","Quality");
        quality["type"]="string_list";quality["is_dynamic"]=false;quality["value"]="Low";quality["index"]=0;
        for(const char* v:{"Low","High"})quality["values"].append(v);
        auto& oldQuality=Option(old,"/graphics/advanced","Quality");
        oldQuality["type"]="string_list";oldQuality["value"]="LocKey#9";oldQuality["index"]=1;
        for(const char* v:{"LocKey#8","LocKey#9","LocKey#10"})oldQuality["values"].append(v);
        auto& zoom=Option(now,"/controls/fppcamerapad","FPP_PadZoomFactor");
        zoom["type"]="float";zoom["value"]=0.5;zoom["min_value"]=0.1;zoom["max_value"]=1.0;
        auto& oldZoom=Option(old,"/controls/fppcamerapad","FPP_PadZoomFactor");oldZoom["type"]="float";oldZoom["value"]=2.0;
        auto& sens=Option(now,"/controls/fppcamerapad","FPP_PadSensitivity");
        sens["type"]="int";sens["value"]=5;sens["min_value"]=1;sens["max_value"]=10;
        auto& oldSens=Option(old,"/controls/fppcamerapad","FPP_PadSensitivity");oldSens["type"]="int";oldSens["value"]=7;
        auto& mfg=Option(now,"/graphics/presets","DLSS_MultiFrameGeneration");
        mfg["type"]="string_list";mfg["is_dynamic"]=true;mfg["value"]="";mfg["index"]=-1;
        auto& oldMfg=Option(old,"/graphics/presets","DLSS_MultiFrameGeneration");
        oldMfg["type"]="string_list";oldMfg["is_dynamic"]=true;oldMfg["value"]="x2";oldMfg["index"]=0;
        auto& dlss=Option(now,"/graphics/presets","DLSS");
        dlss["type"]="string_list";dlss["is_dynamic"]=true;dlss["value"]="Quality";dlss["index"]=2;
        auto& oldDlss=Option(old,"/graphics/presets","DLSS");
        oldDlss["type"]="string_list";oldDlss["is_dynamic"]=true;oldDlss["value"]="Balanced";oldDlss["index"]=3;
        auto& kind=Option(now,"/interface/hud","crosshairs");kind["type"]="bool";kind["value"]=true;
        auto& oldKind=Option(old,"/interface/hud","crosshairs");oldKind["type"]="int";oldKind["value"]=0;
        Option(old,"/controls","SteeringSensitivity")["value"]=0.5;
        Option(old,"/removed/group","Gone")["value"]=1;
        const auto moved=cvr::settings::MergeGameSettings(Encode(now),Encode(old));
        Check(moved.ok,"a 2.31 preset over a 2.5 file failed instead of skipping");
        auto movedJson=Parse(moved.json);
        const auto& s=Option(movedJson,"/controls/controller","InputSchemesMode");
        Check(s["value"]=="Alternative" && s["index"]==2,"a relabelled list was not carried by position");
        Check(Option(movedJson,"/graphics/advanced","Quality")==quality,"a list of another length was written");
        Check(Option(movedJson,"/controls/fppcamerapad","FPP_PadZoomFactor")==zoom,"a number past the player's range was written");
        const auto& written=Option(movedJson,"/controls/fppcamerapad","FPP_PadSensitivity");
        Check(written["value"].isInt() && written["value"]==7,"an int in range was not written as an int");
        Check(Option(movedJson,"/graphics/presets","DLSS_MultiFrameGeneration")==mfg,"an empty dynamic list was given a choice");
        const auto& d=Option(movedJson,"/graphics/presets","DLSS");
        Check(d["value"]=="Balanced" && d["index"]==3,"a dynamic list with a choice did not take the preset's");
        Check(Option(movedJson,"/interface/hud","crosshairs")==kind,"a value of another type was written");
        bool steering=false;
        for(const auto& o:Group(movedJson,"/controls")["options"])steering|=o["name"]=="SteeringSensitivity";
        Check(!steering,"an option the player's file lacks was added");
        bool removed=false;
        for(const auto& g:movedJson["data"])removed|=g["group_name"]=="/removed/group";
        Check(!removed,"a group the player's file lacks was added");
        Check(moved.changed==3 && moved.skipped.size()==6,"2.31 -> 2.5 counts are off");
        Check(cvr::settings::MergeGameSettings(moved.json,Encode(old)).changed==0,"2.31 -> 2.5 merge is not idempotent");
    }
    for(const auto& bad:{std::string("{"),std::string("[]"),std::string(R"({"data":[],"data":[]})"),
        std::string(R"({"data":[{"group_name":"/controls","options":[{"name":"x","value":1},{"name":"x","value":2}]}]})")}){
        Check(!cvr::settings::MergeGameSettings(bad,Encode(preset)).ok,"invalid player settings accepted");
        Check(!cvr::settings::MergeGameSettings(Encode(player),bad).ok,"invalid preset accepted");
    }
    const auto directory=fs::path(argv[2])/(L"settings-\u0416-"+std::to_wstring(GetCurrentProcessId())+L"-"+std::to_wstring(GetTickCount64()));
    fs::create_directory(directory);
    const auto userFile=directory/L"UserSettings.json",sourceFile=directory/L"preset.json";
    const auto original=Encode(before);Write(userFile,original);Write(sourceFile,Encode(preset));
    const auto installed=cvr::settings::InstallGameSettings(sourceFile,userFile);
    Check(installed.ok && installed.changed==expected && !installed.backup.empty(),"first install failed");
    Check(Read(installed.backup)==original,"backup did not preserve exact original bytes");
    Check(Parse(Read(userFile))==Parse(merged.json),"installed result differs from merge");
    const auto again=cvr::settings::InstallGameSettings(sourceFile,userFile);
    Check(again.ok && again.changed==0 && again.backup.empty(),"matching file was unnecessarily rewritten");
    Write(userFile,"{ broken");const auto invalid=Read(userFile);
    Check(!cvr::settings::InstallGameSettings(sourceFile,userFile).ok && Read(userFile)==invalid,"parse failure changed player's file");
    Write(userFile,original);Check(SetFileAttributesW(userFile.c_str(),FILE_ATTRIBUTE_READONLY),"readonly fixture failed");
    const auto locked=cvr::settings::InstallGameSettings(sourceFile,userFile);
    Check(!locked.ok && Read(userFile)==original,"failed replacement changed original file");
    SetFileAttributesW(userFile.c_str(),FILE_ATTRIBUTE_NORMAL);
    // One option set in place: the launch's borderless window.
    Write(userFile,original);
    const auto set=cvr::settings::SetGameSetting(userFile,"/video/display","WindowMode","BorderlessWindowed",1);
    Check(set.ok && set.changed && set.previous=="player_BorderlessWindowed","window mode was not set");
    auto setJson=Parse(Read(userFile));
    Check(Option(setJson,"/video/display","WindowMode")["value"]=="BorderlessWindowed" &&
          Option(setJson,"/video/display","WindowMode")["index"]==1,"window mode written wrong");
    Check(setJson["version"]==999 && setJson["player_metadata"]=="keep","setting one value lost the file's other fields");
    const auto setAgain=cvr::settings::SetGameSetting(userFile,"/video/display","WindowMode","BorderlessWindowed",1);
    Check(setAgain.ok && !setAgain.changed,"a matching value was rewritten");
    const auto beforeMissing=Read(userFile);
    Check(!cvr::settings::SetGameSetting(userFile,"/video/display","NoSuchOption","x",0).ok && Read(userFile)==beforeMissing,
          "a missing option was invented");
    for(const auto& file:fs::directory_iterator(directory))Check(file.path().extension()!=L".tmp","temporary file was not cleaned up");
    if(argc==4)CheckRealFile(fs::path(argv[3]),preset);
    std::cout<<"PASS "<<expected<<" preset values, personal exclusions, unknown entries, metadata, strict JSON, menu buttons, 2.31 -> 2.5 schema, Unicode path, backup, atomic failure, repeat and one setting in place\n";
    return 0;
}catch(const std::exception& e){std::cerr<<e.what()<<'\n';return 1;}
