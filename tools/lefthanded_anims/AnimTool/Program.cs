// AnimTool -- player animation sets (.anims) to glTF and back, in process with WolvenKit's own exporter and importer,
// many sets per run (loading the game's archives is the slow part).
//
//     AnimTool export <game exe> <out dir> <depot path or .anims file>...
//         each set as the game has it: <out dir>\<depot path> (the cooked set) and <out dir>\<depot path>.glb; a
//         set on disk is exported beside itself
//     AnimTool build <game exe> <jobs.txt>
//         each line "template.anims|animations.glb|out.anims": the template's set (its rig binding, its tags) with
//         its own animations removed and the glTF's imported in their place
//
// ADDITIVE ANIMATIONS STAY DELTAS BOTH WAYS. WolvenKit's exporter can bake an additive animation onto the rest pose
// and its importer can take the rest pose back off, but the two are not inverses: a set exported baked and imported
// stripped comes back with its additive animations up to 140 degrees off (measured on the Lexington's add_cover_*).
// So the export here writes the raw deltas (additiveAddRelative = false) and the import adds nothing back
// (AdditiveStripLocalTransform = false): an unchanged set makes the round trip exactly.
//
// WolvenKit's command line cannot build a set at all -- its import takes any .glb for a mesh -- so the importer is
// called with ImportFormat = Anims, the way the WolvenKit editor calls it. The game is loaded for the rig a set or a
// glTF names.
using System.ComponentModel;
using System.Text.Json;
using Microsoft.Extensions.DependencyInjection;
using WolvenKit.Common;
using WolvenKit.Common.Interfaces;
using WolvenKit.Common.Model;
using WolvenKit.Common.Model.Arguments;
using WolvenKit.Common.Services;
using WolvenKit.Core.Compression;
using WolvenKit.Core.Interfaces;
using WolvenKit.Core.Services;
using WolvenKit.Modkit.RED4;
using WolvenKit.Modkit.RED4.Tools;
using WolvenKit.RED4.Archive.CR2W;
using WolvenKit.RED4.Archive.IO;
using WolvenKit.RED4.CR2W;
using WolvenKit.RED4.CR2W.Archive;
using WolvenKit.RED4.Types;

if (args.Length < 3 || (args[0] != "export" && args[0] != "build"))
{
    Console.Error.WriteLine("usage: AnimTool export <game exe> <out dir> <depot path>...\n" +
                            "       AnimTool build <game exe> <jobs.txt>");
    return 2;
}

Oodle.Load();
var services = new ServiceCollection();
services.AddLogging();
services.AddSingleton<ILoggerService>(new ConsoleLogger());
services.AddSingleton<IProgressService<double>>(new SilentProgress());
services.AddSingleton<IHashService, HashService>();
services.AddSingleton<ITweakDBService, TweakDBService>();
services.AddSingleton<ILocKeyService, LocKeyService>();
services.AddSingleton<IHookService, HookService>();
services.AddScoped<IArchiveManager, ArchiveManager>();
services.AddScoped<IModTools, ModTools>();
services.AddScoped<Red4ParserService>();
services.AddScoped<MeshTools>();
services.AddOptions<CommonImportArgs>();
services.AddOptions<XbmImportArgs>();
services.AddOptions<GltfImportArgs>();
using var provider = services.BuildServiceProvider();
using var scope = provider.CreateScope();
var sp = scope.ServiceProvider;
var archives = sp.GetRequiredService<IArchiveManager>();
archives.LoadGameArchives(new FileInfo(args[1]));
var parser = sp.GetRequiredService<Red4ParserService>();
var tools = (ModTools)sp.GetRequiredService<IModTools>();
var failures = 0;

if (args[0] == "export")
{
    var outDir = args[2];
    foreach (var depot in args.Skip(3))
    {
        if (File.Exists(depot))
        {
            using var local = File.OpenRead(depot);
            if (!parser.TryReadRed4File(local, out var own) || own is null ||
                !tools.ExportAnim(own, new FileInfo(depot), true, false, false, SharpGLTF.Validation.ValidationMode.Skip))
            {
                Console.Error.WriteLine($"export failed: {depot}");
                failures++;
            }
            continue;
        }
        var game = archives.GetGameFile(depot, false, false);
        if (game is null)
        {
            Console.Error.WriteLine($"not in the game: {depot}");
            failures++;
            continue;
        }
        var cooked = new FileInfo(Path.Combine(outDir, depot));
        cooked.Directory!.Create();
        using (var stream = cooked.Create())
        {
            game.Extract(stream);
        }
        CR2WFile? file;
        using (var stream = cooked.OpenRead())
        {
            if (!parser.TryReadRed4File(stream, out file) || file is null)
            {
                Console.Error.WriteLine($"unreadable: {depot}");
                failures++;
                continue;
            }
        }
        // the exporter names its output after the file it is given, plus .glb
        if (!tools.ExportAnim(file, cooked, true, false, false, SharpGLTF.Validation.ValidationMode.Skip) ||
            !File.Exists(cooked.FullName + ".glb"))
        {
            Console.Error.WriteLine($"export failed: {depot}");
            failures++;
            continue;
        }
        Console.WriteLine($"exported {depot}");
    }
    return failures == 0 ? 0 : 1;
}

foreach (var line in File.ReadAllLines(args[2]))
{
    var parts = line.Split('|');
    if (parts.Length != 3)
    {
        continue;
    }
    var template = new FileInfo(parts[0]);
    var glb = new FileInfo(parts[1]);
    var output = new FileInfo(parts[2]);

    // The template, emptied: no animation, no data chunk, no fallback frames.
    CR2WFile? file;
    using (var stream = template.OpenRead())
    {
        if (!parser.TryReadRed4File(stream, out file) || file?.RootChunk is not animAnimSet)
        {
            Console.Error.WriteLine($"{template.FullName}: not an animation set");
            failures++;
            continue;
        }
    }
    var set = (animAnimSet)file.RootChunk;
    set.Animations.Clear();
    set.AnimationDataChunks.Clear();
    set.FallbackDataAddresses.Clear();
    set.FallbackDataAddressIndexes.Clear();
    set.FallbackAnimFrameDescs.Clear();
    set.FallbackAnimDescIndexes.Clear();
    set.FallbackAnimDataBuffer = new DataBuffer();
    set.FallbackNumPositionData = 0;
    set.FallbackNumRotationData = 0;
    set.FallbackNumFloatTrackData = 0;
    output.Directory!.Create();
    using (var stream = output.Create())
    using (var writer = new CR2WWriter(stream))
    {
        writer.WriteFile(file);
    }

    // The glTF beside it under the same name, imported into it.
    var raw = Path.Combine(output.DirectoryName!, Path.GetFileNameWithoutExtension(output.Name) + ".glb");
    File.Copy(glb.FullName, raw, true);
    var importArgs = new GlobalImportArgs().Register(new CommonImportArgs(), new XbmImportArgs(), new GltfImportArgs());
    importArgs.Get<CommonImportArgs>().Keep = true;
    var gltf = importArgs.Get<GltfImportArgs>();
    gltf.Keep = true;
    gltf.ImportFormat = GltfImportAsFormat.Anims;
    gltf.AdditiveStripLocalTransform = false;
    var ok = await tools.Import(new RedRelativePath(output.Directory, Path.GetFileName(raw)), importArgs, output.Directory);
    File.Delete(raw);
    if (!ok)
    {
        Console.Error.WriteLine($"import failed: {glb.FullName}");
        failures++;
        continue;
    }
    // THE MIRRORED ANIMATIONS' OWN FALLBACK FRAMES. A cooked set carries a few frames of its animations outside their
    // buffers (the set's fallback tables) for the engine to pose with until the data streams in, and an animation
    // names its own among them (fallbackFrameIndices). The template's tables are the right hand's and were emptied
    // above; mirror_anims encoded the left hand's beside the glTF (<glb>.fallback.json), and the importer kept each
    // animation's indices into them from the glTF. An index past the tables is dropped.
    CR2WFile? built;
    using (var stream = output.OpenRead())
    {
        if (!parser.TryReadRed4File(stream, out built) || built?.RootChunk is not animAnimSet)
        {
            Console.Error.WriteLine($"{output.FullName}: unreadable after import");
            failures++;
            continue;
        }
    }
    var result = (animAnimSet)built.RootChunk;
    var fallbackFrames = ApplyFallback(result, glb.FullName + ".fallback.json");
    // ...AND ALL THE DATA IN ONE CHUNK. The importer gives each animation a data chunk of its own, which the engine
    // streams in when that animation is first asked for; a cooked set covers that wait with its fallback frames,
    // these have none. One chunk comes in whole with the first animation asked for -- the draw's -- so the hold, the
    // rounds and the reload that follow it find their data already in.
    // Every buffer is moved, whatever its kind (the importer writes the long ones -- reloads, first_equip -- as SIMD
    // buffers): the engine indexes the set's chunk table with a buffer's chunk index unchecked, and one left pointing
    // past the single chunk hung the game the first time that animation was asked for.
    // Each buffer starts on 16 bytes, as it did at the start of its own chunk: the engine samples a SIMD buffer with
    // aligned SSE loads (Cyberpunk2077.exe 2.5, sub_1405E1A44 subps on the key data), and one packed behind a
    // compressed buffer of odd length faulted the first time its frames were blended.
    var data = new MemoryStream();
    var moved = true;
    foreach (var entry in result.Animations)
    {
        var buffer = entry.Chunk?.Animation.Chunk?.AnimBuffer.Chunk;
        animAnimDataAddress? address;
        switch (buffer)
        {
            case animAnimationBufferCompressed compressed:
                KeepFallback(compressed.FallbackFrameIndices, fallbackFrames);
                address = compressed.DataAddress;
                break;
            case animAnimationBufferSimd simd:
                KeepFallback(simd.FallbackFrameIndices, fallbackFrames);
                address = simd.DataAddress;
                break;
            default:
                Console.Error.WriteLine($"{output.Name}: {entry.Chunk?.Animation.Chunk?.Name}: buffer {buffer?.GetType().Name} not moved");
                moved = false;
                continue;
        }
        if (address is null || (uint)address.UnkIndex >= (uint)result.AnimationDataChunks.Count)
        {
            Console.Error.WriteLine($"{output.Name}: {entry.Chunk?.Animation.Chunk?.Name}: no data chunk {address?.UnkIndex}");
            moved = false;
            continue;
        }
        var bytes = result.AnimationDataChunks[(int)(uint)address.UnkIndex].Buffer.Buffer.GetBytes();
        var offset = (uint)((data.Length + 15) & ~15L);
        data.SetLength(offset);
        data.Position = offset;
        data.Write(bytes, (int)(uint)address.FsetInBytes, (int)(uint)address.ZeInBytes);
        address.UnkIndex = 0;
        address.FsetInBytes = offset;
    }
    if (!moved)
    {
        output.Delete();
        failures++;
        continue;
    }
    if (result.AnimationDataChunks.Count > 0)
    {
        var whole = result.AnimationDataChunks[0];
        whole.Buffer = new SerializationDeferredDataBuffer(data.ToArray());
        result.AnimationDataChunks.Clear();
        result.AnimationDataChunks.Add(whole);
    }
    using (var stream = output.Create())
    using (var writer = new CR2WWriter(stream))
    {
        writer.WriteFile(built);
    }
    Console.WriteLine($"built {output.Name}: {result.Animations.Count} animations, {fallbackFrames} fallback frames");
}
return failures == 0 ? 0 : 1;

// The set's fallback tables from mirror_anims' encoding; the number of frames (0 when there is none).
static int ApplyFallback(animAnimSet set, string path)
{
    set.FallbackDataAddresses.Clear();
    set.FallbackDataAddressIndexes.Clear();
    set.FallbackAnimFrameDescs.Clear();
    set.FallbackAnimDescIndexes.Clear();
    set.FallbackAnimDataBuffer = new DataBuffer();
    set.FallbackNumPositionData = 0;
    set.FallbackNumRotationData = 0;
    set.FallbackNumFloatTrackData = 0;
    if (!File.Exists(path))
    {
        return 0;
    }
    using var doc = JsonDocument.Parse(File.ReadAllText(path));
    var root = doc.RootElement;
    set.FallbackAnimDataBuffer = new DataBuffer(Convert.FromHexString(root.GetProperty("buffer").GetString() ?? ""));
    foreach (var d in root.GetProperty("frameDescs").EnumerateArray())
    {
        set.FallbackAnimFrameDescs.Add(new animAnimFallbackFrameDesc
        {
            MPositions = d[0].GetUInt16(), MRotations = d[1].GetUInt16(), MFloatTracks = d[2].GetUInt16()
        });
    }
    foreach (var a in root.GetProperty("dataAddresses").EnumerateArray())
    {
        set.FallbackDataAddresses.Add((CUInt16)a.GetUInt16());
    }
    foreach (var i in root.GetProperty("descIndexes").EnumerateArray())
    {
        set.FallbackAnimDescIndexes.Add((CUInt8)i.GetByte());
    }
    foreach (var i in root.GetProperty("addressIndexes").EnumerateArray())
    {
        set.FallbackDataAddressIndexes.Add((CUInt8)i.GetByte());
    }
    set.FallbackNumPositionData = root.GetProperty("numPositions").GetUInt16();
    set.FallbackNumRotationData = root.GetProperty("numRotations").GetUInt16();
    set.FallbackNumFloatTrackData = root.GetProperty("numFloatTracks").GetUInt16();
    return set.FallbackAnimDescIndexes.Count;
}

static void KeepFallback(CArray<CUInt16> indices, int frames)
{
    if (indices.Any(i => (ushort)i >= frames))
    {
        indices.Clear();
    }
}

sealed class ConsoleLogger : ILoggerService
{
    public LoggerVerbosity LoggerVerbosity { get; private set; } = LoggerVerbosity.Normal;
    public void SetLoggerVerbosity(LoggerVerbosity verbosity) => LoggerVerbosity = verbosity;
    public void Info(string s) { }
    public void Debug(string s) { }
    public void Warning(string s) => Console.WriteLine($"warning: {s}");
    public void Error(string s) => Console.Error.WriteLine($"error: {s}");
    public void Error(Exception e) => Console.Error.WriteLine($"error: {e.Message}");
    public void Success(string s) { }
}

#pragma warning disable CS0067
sealed class SilentProgress : IProgressService<double>
{
    public event EventHandler<double>? ProgressChanged;
    public event PropertyChangedEventHandler? PropertyChanged;
    public bool IsIndeterminate { get; set; }
    public EStatus Status { get; set; }
    public void Completed() { }
    public void Report(double value) { }
}
#pragma warning restore CS0067
