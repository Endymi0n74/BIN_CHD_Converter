using System.Security.Cryptography;
using FormatHelper.Formats.Ecm;

namespace BatchConvertToCHD.Tests;

public sealed class EcmImageDecoderTests
{
    private const int ExpectedDecodedLength = 28224;
    private const string ExpectedSha1 = "C79042C9DF371FDED431F72B43DCBEDC4DEAEF11";

    [Fact]
    public async Task ReferenceFixtureDecodesToExpectedImage()
    {
        var fixturePath = Path.Combine(AppContext.BaseDirectory, "Fixtures", "ecm-sample.ecm");
        Assert.True(File.Exists(fixturePath), $"ECM fixture is missing: {fixturePath}");

        var temporaryDirectory = Path.Combine(Path.GetTempPath(), $"EcmImageDecoderTests_{Guid.NewGuid():N}");
        Directory.CreateDirectory(temporaryDirectory);
        try
        {
            var outputPath = Path.Combine(temporaryDirectory, "decoded.bin");
            var result = await EcmImageDecoder.DecodeAsync(
                fixturePath,
                outputPath,
                _ => { },
                CancellationToken.None
            );

            Assert.True(result.Success, result.FailureReason);
            Assert.True(File.Exists(outputPath));

            var decoded = await File.ReadAllBytesAsync(outputPath);
            Assert.Equal(ExpectedDecodedLength, decoded.Length);
            Assert.Equal(ExpectedSha1, Convert.ToHexString(SHA1.HashData(decoded)));
            Assert.Equal(decoded.Length, result.BytesWritten);
        }
        finally
        {
            try
            {
                Directory.Delete(temporaryDirectory, recursive: true);
            }
            catch
            {
                // Do not mask a test failure with best-effort temporary-file cleanup.
            }
        }
    }
}