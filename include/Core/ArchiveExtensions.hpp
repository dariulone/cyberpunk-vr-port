#pragma once

// THE PORT'S ARCHIVE, WITHOUT ARCHIVEXL (stage 3 of docs/native-frameworks-plan-20261030.md).
//
// The port's assets are archive\cyberpunkvrport.archive in the plugin's folder, mounted by the plugin as the resource
// depot initializes its archives -- in a mod group of its own, the way the game mounts archive\pc\mod -- and its
// manifest, config\archive.yaml (ArchiveXL's format; the repository's mods\archive\cyberpunkvrport.archive.xl), is
// applied by the plugin too:
//
//   factories   item factory CSVs, loaded after the game's own (the belt, the props, the cigarette)
//   animations  animation sets added to an entity's animated component, with their variables and priority (the
//               left-handed weapon sets on the player's root)
//   resource    patches: a streaming block merged into a streaming world (the lobby in the main menu's world), and
//               mesh appearances added to meshes (the construct)
//
// and two engine changes ArchiveXL makes for every mod, which the port's assets were made with: world widget
// components past the tenth of a layer are updated, and an entity spawned from a record takes its appearanceName.
//
// ArchiveXL itself (another mod's) is untouched: it looks for manifests (*.xl) in every mod group's folder, so the
// port's is kept out of the archive folder; its hooks and the port's chain on the same engine functions; and copies
// an earlier release put in archive\pc\mod are taken out at load -- before the depot lists that folder, and before
// ArchiveXL looks for manifests -- so nothing is mounted or applied twice.
#include <RED4ext/Api/v1/PluginHandle.hpp>
#include <RED4ext/Api/v1/Sdk.hpp>

namespace RED4ext { struct ISerializable; }

namespace cvr::archive {

// A loaded resource of a native type set right as it loads -- its dependencies in, before anything uses it -- by a
// feature of the port's whose change depends on the player (the boot's notice in the game's language: Lobby/
// Notice.hpp). Added at load, before Install; the fix runs on the resource serializer's jobs.
using Fixup = void (*)(RED4ext::ISerializable* resource);
void AddFixup(const char* type, Fixup fix);

// At load: takes earlier releases' copies out of archive\pc\mod, reads the manifest and hooks the engine paths.
void Install(RED4ext::v1::PluginHandle handle, const RED4ext::v1::Sdk* sdk);

}  // namespace cvr::archive
