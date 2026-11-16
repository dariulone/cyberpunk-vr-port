#pragma once
namespace cvr::cyberfinger {
void ToggleHud();
void SetHudHidden(bool);
bool HudHidden();
// Null unless an active context temporarily overrides the hidden preference.
const char* HudRevealReason();
bool HudVisible();
float HudScale();
enum class Command { None, Inventory, Pause, Cyberware, MeleeToggle };
void RequestCommand(Command);
Command TakeCommand();
enum class MenuContext { General, Map, Journal, PictureFrame };
void UiTick(MenuContext context,bool interaction);
MenuContext Context();
bool InteractionActive();
void LootTick(bool visible);
void SubtitleTick(bool visible);
// Called by the game-thread script only while its active choice hub is visible.
int ConversationTick(bool visible);
bool ConversationActive();
void ConversationInput(bool enabled,float rightY,float rightTrigger);
}
