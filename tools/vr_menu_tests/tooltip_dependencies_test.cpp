#include "Natives/MenuTooltipDependencies.hpp"
#include <cassert>
#include <memory>
#include <vector>
#include <iostream>

struct Resource {};
struct Dependency { int path{}; std::shared_ptr<Resource> token; };
struct Dependencies : std::vector<Dependency> {
    void PushBack(const Dependency& value) { push_back(value); }
};

int main() {
    using cvr::menu_tooltip::DeclareReadyLibrary;
    Dependencies owner;
    auto token = std::make_shared<Resource>();
    std::weak_ptr<Resource> weak = token;
    DeclareReadyLibrary<Dependency>(owner, 1, token);
    assert(owner.size() == 1 && owner[0].path == 1 && owner[0].token == token);
    for (int i = 0; i < 1000; ++i) DeclareReadyLibrary<Dependency>(owner, 1, token);
    assert(owner.size() == 1);
    token.reset();
    assert(!weak.expired()); // Child modules still resolve after the scoped spawn/cache reference leaves.
    auto replacement = std::make_shared<Resource>();
    DeclareReadyLibrary<Dependency>(owner, 1, replacement);
    assert(owner.size() == 1 && owner[0].token == replacement && weak.expired());
    owner.PushBack({2, {}}); // A declared but not loaded dependency is reused.
    DeclareReadyLibrary<Dependency>(owner, 2, replacement);
    assert(owner.size() == 2 && owner[1].token == replacement);
    Dependencies other;
    DeclareReadyLibrary<Dependency>(other, 1, replacement);
    owner.clear();
    assert(other.size() == 1 && other[0].token == replacement);
    std::cout << "PASS tooltip library registration, deduplication, missing-token repair, independent owners, lifetime\n";
}
