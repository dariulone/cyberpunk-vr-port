#pragma once
namespace cvr::cyberfinger {
void ToggleHud();
bool HudVisible();
float HudScale();
enum class Command { None, Inventory, Pause, Cyberware };
void RequestCommand(Command);
Command TakeCommand();
enum class MenuContext { General, Map, Journal, PictureFrame };
void UiTick(MenuContext context,bool interaction);
MenuContext Context();
bool InteractionActive();
void LootTick(bool visible);
// Called by the game-thread script only while its active choice hub is visible.
int ConversationTick(bool visible);
bool ConversationActive();
void ConversationInput(bool enabled,float rightY,float rightTrigger);
}
