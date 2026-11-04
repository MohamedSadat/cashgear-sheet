// Validates workbooks with the Open XML SDK, one line of JSON each. Paths come from a list file,
// and results refer to them only by their place in it, so private files leave no names.
// Usage: Validator <list.txt> <out.jsonl> [parallel]
using System.Collections.Concurrent;
using System.Text.Json;
using DocumentFormat.OpenXml;
using DocumentFormat.OpenXml.Packaging;
using DocumentFormat.OpenXml.Validation;

var paths = File.ReadAllLines(args[0]).Where(line => line.Length > 0).ToArray();
var parallel = args.Length > 2 ? int.Parse(args[2]) : Environment.ProcessorCount;
var results = new ConcurrentDictionary<int, string>();

Parallel.For(0, paths.Length, new ParallelOptions { MaxDegreeOfParallelism = parallel }, index =>
{
    var counts = new Dictionary<string, int>();
    var samples = new Dictionary<string, string>();
    string? openError = null;
    try
    {
        using var document = SpreadsheetDocument.Open(
            paths[index],
            false,
            new OpenSettings { AutoSave = false }
        );
        // Whatever Excel is current, so nothing passes for being too new.
        var validator = new OpenXmlValidator(FileFormatVersions.Microsoft365) { MaxNumberOfErrors = 0 };
        foreach (var error in validator.Validate(document))
        {
            var key = (error.Id ?? "?") + "|" + (error.Part?.Uri.ToString() ?? "?");
            counts[key] = counts.GetValueOrDefault(key) + 1;
            if (!samples.ContainsKey(key))
            {
                samples[key] = (error.Path?.XPath ?? "") + " " + error.Description;
            }
        }
    }
    catch (Exception error)
    {
        openError = error.GetType().Name + ": " + error.Message;
    }
    results[index] = JsonSerializer.Serialize(
        new Dictionary<string, object?>
        {
            ["index"] = index,
            ["openError"] = openError,
            ["errors"] = counts,
            ["samples"] = samples,
        }
    );
});

using var output = new StreamWriter(args[1]);
foreach (var (_, line) in results.OrderBy(pair => pair.Key))
{
    output.WriteLine(line);
}
