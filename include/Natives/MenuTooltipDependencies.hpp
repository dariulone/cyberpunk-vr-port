#pragma once

namespace cvr::menu_tooltip {
// Called with the dependency write lock held. The owner keeps the ready token so both the initial
// external spawn and its controllers' later local-module spawns resolve the same originating library.
template<class Dependency, class Dependencies, class Path, class Token>
void DeclareReadyLibrary(Dependencies& dependencies, const Path& path, const Token& token) {
    for (auto& dependency : dependencies) {
        if (dependency.path != path) continue;
        dependency.token = token;
        return;
    }
    Dependency dependency;
    dependency.path = path;
    dependency.token = token;
    dependencies.PushBack(dependency);
}
}
