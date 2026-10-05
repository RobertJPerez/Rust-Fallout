// Separate GPL-3.0-only offline comparison; no runtime dependency.
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using Mutagen.Bethesda;
using Mutagen.Bethesda.Archives;

if (args.Length != 1) throw new ArgumentException("usage: archive-oracle CENSUS_JSON");
using var census = JsonDocument.Parse(File.ReadAllText(args[0]));
var results = new List<object>();
bool passed = true;
long totalScripts = 0, totalScriptBytes = 0, totalEntries = 0;
foreach (var expected in census.RootElement.GetProperty("archives").EnumerateArray())
{
    string path = expected.GetProperty("file").GetString()!;
    Console.Error.WriteLine("Independently reading " + Path.GetFileName(path));
    using var guard = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.Read);
    string sourceSha = Convert.ToHexStringLower(SHA256.HashData(guard));
    bool sourceMatch = sourceSha == expected.GetProperty("sha256").GetString() && guard.Length == expected.GetProperty("bytes").GetInt64();
    if (!sourceMatch) throw new InvalidDataException("source changed since census: " + path);
    var archive = Archive.CreateReader(GameRelease.SkyrimSE, path);
    var expectedScripts = expected.GetProperty("scripts").EnumerateArray().ToDictionary(s =>
        Encoding.ASCII.GetString(s.GetProperty("path").EnumerateArray().Select(b => b.GetByte()).ToArray()), s => s);
    var seen = new HashSet<string>(StringComparer.Ordinal);
    var matchedScripts = new HashSet<string>(StringComparer.Ordinal);
    var extensions = new SortedDictionary<string, long>(StringComparer.Ordinal);
    var failures = new List<object>();
    int entries = 0, duplicates = 0, scripts = 0, mismatches = 0;
    long scriptBytes = 0;
    foreach (var file in archive.Files)
    {
        // The installed corpus is ASCII. Do not silently normalize unverified encodings.
        if (file.Path.Any(c => c > 127)) throw new InvalidDataException("non-ASCII archive path needs a separate encoding comparison");
        string asset = file.Path.Replace('\\', '/').ToLowerInvariant();
        if (!seen.Add(asset)) duplicates++;
        entries++;
        string extension = asset.Split('.').Last();
        extensions[extension] = extensions.GetValueOrDefault(extension) + 1;
        if (!asset.StartsWith("scripts/", StringComparison.Ordinal) || !asset.EndsWith(".pex", StringComparison.Ordinal)) continue;
        scripts++;
        if (file.Size > 64 * 1024 * 1024) throw new InvalidDataException("script exceeds extraction bound");
        using var payload = file.AsStream();
        using var hash = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
        byte[] buffer = new byte[64 * 1024];
        long bytes = 0;
        int n;
        while ((n = payload.Read(buffer)) > 0)
        {
            bytes += n;
            scriptBytes += n;
            if (bytes > 64 * 1024 * 1024 || scriptBytes > 512 * 1024 * 1024) throw new InvalidDataException("script decompression budget exceeded");
            hash.AppendData(buffer, 0, n);
        }
        string digest = Convert.ToHexStringLower(hash.GetHashAndReset());
        bool match = expectedScripts.TryGetValue(asset, out var wanted) && wanted.GetProperty("bytes").GetInt64() == bytes && wanted.GetProperty("sha256").GetString() == digest;
        if (match) matchedScripts.Add(asset);
        else
        {
            mismatches++;
            if (failures.Count < 64) failures.Add(new { path = asset, bytes, sha256 = digest });
        }
    }
    bool entriesMatch = entries == expected.GetProperty("entries").GetInt32();
    bool duplicatesMatch = duplicates == expected.GetProperty("duplicate_paths").GetInt32();
    var expectedExtensions = expected.GetProperty("extensions").EnumerateObject().ToDictionary(p => p.Name, p => p.Value.GetInt64());
    bool extensionsMatch = extensions.Count == expectedExtensions.Count && extensions.All(p => expectedExtensions.TryGetValue(p.Key, out long value) && value == p.Value);
    var missing = expectedScripts.Keys.Except(matchedScripts).ToArray();
    bool archivePassed = entriesMatch && duplicatesMatch && extensionsMatch && mismatches == 0 && missing.Length == 0 && scripts == expectedScripts.Count;
    passed &= archivePassed;
    totalScripts += scripts;
    totalScriptBytes += scriptBytes;
    totalEntries += entries;
    results.Add(new {
        file = Path.GetFileName(path), sha256 = sourceSha, passed = archivePassed,
        entries, scripts, matched_scripts = matchedScripts.Count, script_bytes = scriptBytes,
        entries_match = entriesMatch, duplicate_paths_match = duplicatesMatch,
        extensions_match = extensionsMatch, mismatches, missing, examples = failures
    });
}
if (results.Count == 0) throw new InvalidDataException("no archives to compare");
Console.WriteLine(JsonSerializer.Serialize(new {
    schema_version = 1, oracle = "Mutagen.Bethesda.Core", version = "0.54.4",
    revision = "0188012c607ce8bb283d2704400d37737f089134", passed,
    archives = results.Count, entries = totalEntries, scripts = totalScripts, script_bytes = totalScriptBytes, comparisons = results,
    scope = "Entry count, extension histogram, duplicate path count, and every script path/uncompressed length/SHA-256. Header fields, non-script payload contents and runtime mounting precedence are not certified."
}, new JsonSerializerOptions { WriteIndented = true }));
return passed ? 0 : 2;
