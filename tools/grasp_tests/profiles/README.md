# Grasp profiles of round props

A prop's collider in the game is usually a capsule round the whole of it. A bottle's capsule has no neck and is thicker
than the glass, so the grasp (src/Anim/GraspPose.cpp, GraspPartOf) reads where the neck is, how thin it is and how thick
the body really is from the mesh the prop is drawn with. src/Natives/HeldBody.cpp finds the held body's CMesh, looks its
depot path's FNV1a64 up in src/Anim/GraspProfiles.inc and turns the profile into rings of points in the capsule's frame.

The table is generated, not written by hand:

1. Extract the cooked meshes (WolvenKit, find_and_extract) into build/grasp_profiles/raw, e.g. every
   `decoration|props ... *bottle*.mesh` and every mesh under `decoration\food\drinks`.
2. Convert them to JSON (WolvenKit cr2w_to_json) into build/grasp_profiles/json.
3. `py -3.13 -B tools/grasp_tests/profiles/pack.py build/grasp_profiles/raw build/grasp_profiles/json build/grasp_profiles/pack.bin`
   lays out each mesh's LOD0 position streams (Short4N) and triangle lists, untouched.
4. `tools/grasp_tests/profiles/build.bat`, then
   `tools/grasp_tests/profiles/profilegen.exe build/grasp_profiles/pack.bin src/Anim/GraspProfiles.inc`
   decodes the positions, keeps the long round meshes and writes each one's radius round its long axis in 16 slices
   (read along every triangle edge every 2 mm). Its report lists every mesh and whether it has a neck.

To cover more props, extract more meshes in step 1 and run the steps again.
