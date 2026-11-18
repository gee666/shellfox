using System.Text.Json;
using Shellfox.ElevatedTerminal;
using Xunit;

namespace Shellfox.ElevatedTerminal.Tests;

public sealed class PipeRunArgumentTests
{
    private static PipeRunConfig Config(params string[] args) => PipeRunConfig.Parse(JsonSerializer.SerializeToUtf8Bytes(new
    {
        executable = @"C:\Windows\System32\cmd.exe", args, cwd = @"C:\owned", env = new Dictionary<string, string>(),
    }));
    [Fact]
    public void ExecutableIsAlwaysQuotedAndOnlyUnambiguousAsciiTokensAreBare()
    {
        Assert.Equal("\"C:\\Windows\\System32\\cmd.exe\" /d /q /c \"exit /b 23\"", Config("/d", "/q", "/c", "exit /b 23").CommandLine());
        Assert.Equal("\"C:\\Windows\\System32\\cmd.exe\" plain C:\\owned\\ --flag a_b-1.exe", Config("plain", @"C:\owned\", "--flag", "a_b-1.exe").CommandLine());
        Assert.Equal("\"C:\\Windows\\System32\\cmd.exe\"", Config().CommandLine());
    }
    [Theory]
    [InlineData("")]
    [InlineData("space value")]
    [InlineData("tab\tvalue")]
    [InlineData("line\nvalue")]
    [InlineData("雪🙂")]
    [InlineData("quote\"value")]
    [InlineData("a&b")]
    [InlineData("a|b")]
    [InlineData("a>b")]
    [InlineData("a<b")]
    [InlineData("a^b")]
    [InlineData("%PATH%")]
    [InlineData("!value!")]
    [InlineData("(value)")]
    [InlineData("a=b")]
    [InlineData("*.exe")]
    public void AmbiguousArgumentsKeepTheExistingQuoteEncoding(string value) =>
        Assert.Equal("\"C:\\Windows\\System32\\cmd.exe\" " + PipeRunConfig.Quote(value), Config(value).CommandLine());
    [Fact]
    public void QuoteItselfIsUnchangedIncludingTrailingSlashes()
    {
        Assert.Equal("\"\"", PipeRunConfig.Quote(""));
        Assert.Equal("\"a\\\\\"", PipeRunConfig.Quote("a\\"));
        Assert.Equal("\"a\\\"b\"", PipeRunConfig.Quote("a\"b"));
    }
}
