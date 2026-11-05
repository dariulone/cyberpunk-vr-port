#pragma once

// THE PORT'S INPUT BINDINGS, WITHOUT INPUT LOADER (stage 1 of docs/native-frameworks-plan-20261030.md).
//
// The game reads its input contexts and its key mappings from two XML files named by two engine config
// variables, Player/Input InputContextFile and InputMappingFile (defaults config\inputContexts.xml and
// config\inputUserMappings.xml, relative to r6\). The port's own bindings (config\input\*.xml: the scanner HUD
// editor, the scanner zoom keys, the iconic cyberware key) are merged into copies of those two files under the
// rules Input Loader uses -- each <bindings> child goes to the file its kind belongs in, a node with the same
// key (name, action or id) is replaced, or added to with append="true" -- and the two variables are pointed at the
// copies through the variables' own setter, in memory: nothing is written outside the plugin's folder, and nothing
// is left behind when the port is removed.
//
// With Input Loader installed (another mod's) the variables are its: the port's files go to r6\input\ for it to
// merge instead, and the port does not touch the variables.
namespace cvr::input_config {

// At load, before the engine reads its input configuration.
void Apply();

}  // namespace cvr::input_config
