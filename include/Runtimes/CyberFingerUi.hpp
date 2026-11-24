#pragma once
namespace cvr::cyberfinger {
void ToggleHud();
void SetHudHidden(bool);
bool HudHidden();
bool OffensiveMode();
bool NoteFistsEquipped(bool equipped);
bool FistsEquipped();
void SetFistBlock(bool active);
void SetOffensiveMode(bool enabled);
// Null unless an active context temporarily overrides the hidden preference.
const char* HudRevealReason();
enum class ModeNotice { None, Off, On, Holstering };
void RevealModeNotice(ModeNotice notice = ModeNotice::None);
const char* ModeNoticeText();
bool HudVisible();
float HudScale();
enum class Command { None, Inventory, Pause, Cyberware, MeleeToggle, ToggleWornGear, DrawFists, CloseShard };
void RequestCommand(Command);
Command TakeCommand();
enum class MenuContext { General, Map, Journal, PictureFrame, ShardReader };
void UiTick(MenuContext context,bool interaction);
MenuContext Context();
bool InteractionActive();
void LootTick(bool visible);
void SubtitleTick(bool visible);
// Called by the game-thread script only while its active choice hub is visible.
int ConversationTick(bool visible);
bool ConversationActive();
void ConversationInput(bool enabled,float rightY,float leftTrigger,float rightTrigger,bool driverClick);
}
