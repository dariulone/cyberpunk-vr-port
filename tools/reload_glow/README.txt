Bolt and handle glow geometry

Build the C++ generator:
  cmake -S tools/reload_glow -B build/reload-glow/part-generator -A x64
  cmake --build build/reload-glow/part-generator --config Release
  ctest --test-dir build/reload-glow/part-generator -C Release --output-on-failure

Run reload_glow_part PLAN.json OUTPUT.glb REPORT.json. A plan contains frame
(bone name), bones (selected bone names), meshes (WolvenKit mesh JSON paths),
name and optional inflate in metres (default 0.0015). Paths may be relative to
the plan. Only LOD0 triangles with every corner weighted over 0.5 to the selected
bones are retained. Conversion to the bone frame, normals, welded shell and
game-to-glTF coordinates are calculated in C++. Per-instance streams are not
vertex data. Exit 3 means no geometry for this part; malformed data fails.

The Python bolts() workflow now invokes this compiled generator and only
orchestrates files and WolvenKit import. Magazine inflation is a separate legacy
path. A slide.glowBone override selects just that bone and its own vrp_ slot,
instead of the slide plus its rigid riders. Grad uses bone 16 (handle), whose
parent is chamber_opener: the highlight follows the handle independently.

For the Grad correction the current game base mesh supplied 756 handle triangles
and 564 vertices. The resulting 71 x 17 x 18 mm shell is drawn on vrp_handle.
The original glow appearances/materials are preserved on re-import. The archive
was rebuilt from the current archive's extracted contents; an unpacked hash
comparison verified the handle mesh was the only changed resource out of 1791.
