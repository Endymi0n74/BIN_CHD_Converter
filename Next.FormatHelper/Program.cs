using CCDSharp;
using PBPSharp;
using PBPSharp.Models;

if (args.Length != 3 || args[0] is not ("pbp" or "ccd"))
{
    Console.Error.WriteLine("Usage: batch-format-helper <pbp|ccd> <input> <output-directory>");
    return 2;
}

var mode = args[0];
var input = Path.GetFullPath(args[1]);
var output = Path.GetFullPath(args[2]);
Directory.CreateDirectory(output);

try
{
    if (mode == "ccd")
    {
        var cue = Path.Combine(output, Path.GetFileNameWithoutExtension(input) + ".cue");
        Console.WriteLine(CcdConverter.ConvertToCueBin(input, cue, copyBinFile: true));
        return 0;
    }

    var error = PbpFile.Open(input, out var pbp);
    if (error != PbpError.None || pbp is null) throw new InvalidDataException($"Unable to open PBP: {error}");
    using (pbp)
    {
        foreach (var disc in pbp.Discs)
        {
            var suffix = pbp.IsMultiDisc ? $" - Disc {disc.Index}" : "";
            var bin = Path.Combine(output, Path.GetFileNameWithoutExtension(input) + suffix + ".bin");
            var cue = Path.ChangeExtension(bin, ".cue");
            var result = disc.ExtractToBinCue(bin, cue);
            if (result != PbpError.None) throw new InvalidDataException($"Unable to extract disc {disc.Index}: {result}");
            Console.WriteLine(cue);
        }
    }
    return 0;
}
catch (Exception ex)
{
    Console.Error.WriteLine(ex.Message);
    return 1;
}
