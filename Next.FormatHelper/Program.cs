using CCDSharp;
using PBPSharp;
using PBPSharp.Models;
using CSOSharp;
using CSOSharp.Models;
using FormatHelper.Formats.Ecm;
using FormatHelper.Formats.Mds;

if (args.Length != 3 || args[0] is not ("pbp" or "ccd" or "cso" or "ecm" or "mds"))
{
    Console.Error.WriteLine("Usage: batch-format-helper <pbp|ccd|cso|ecm|mds> <input> <output-directory>");
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

    if (mode == "cso")
    {
        var iso = Path.Combine(output, Path.GetFileNameWithoutExtension(input) + ".iso");
        var openResult = CsoFile.Open(input, out var cso);
        if (openResult != CsoError.None || cso is null) throw new InvalidDataException($"Unable to open CSO: {openResult}");
        using (cso)
        {
            var result = cso.ExtractToIso(iso);
            if (result != CsoError.None) throw new InvalidDataException($"Unable to extract CSO: {result}");
        }
        Console.WriteLine(iso);
        return 0;
    }

    if (mode == "ecm")
    {
        var decoded = Path.Combine(output, EcmImageDecoder.GetDecodedFileName(input));
        var result = EcmImageDecoder.DecodeAsync(input, decoded, Console.Error.WriteLine, default)
            .GetAwaiter().GetResult();
        if (!result.Success) throw new InvalidDataException($"Unable to decode ECM: {result.FailureReason}");
        Console.WriteLine(decoded);
        return 0;
    }

    if (mode == "mds")
    {
        var disc = MdsParser.Parse(input);
        var prepared = MdsInputPreparer.PrepareAsync(disc, output, Console.Error.WriteLine, default)
            .GetAwaiter().GetResult();
        if (!prepared.Success) throw new InvalidDataException($"Unable to prepare MDS: {prepared.FailureReason}");
        var target = prepared.CuePath ?? prepared.DvdImagePath;
        if (target is null) throw new InvalidDataException("MDS preparation produced no convertible output");
        Console.WriteLine(target);
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
