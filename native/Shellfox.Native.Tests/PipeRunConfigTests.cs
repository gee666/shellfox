using System.Text;
using System.Text.Json;
using Shellfox.ElevatedTerminal;
using Xunit;

namespace Shellfox.Native.Tests;

public sealed class PipeRunConfigTests
{
    private static byte[] Config(object? args = null, object? env = null) => JsonSerializer.SerializeToUtf8Bytes(new {
        executable = @"C:\Windows\System32\cmd.exe", args = args ?? Array.Empty<string>(), cwd = @"C:\work", env = env ?? new Dictionary<string, string>()
    });

    [Fact]
    public void PreservesArgumentsAndEnvironmentAsData()
    {
        var config = PipeRunConfig.Parse(Config(new[] { "", "spaces 雪", "a&b", "quotes\"", "trailing\\" }, new { PATH = @"C:\tools", TOKEN = "a=b\nsecond line" }));
        Assert.Equal(new[] { "", "spaces 雪", "a&b", "quotes\"", "trailing\\" }, config.Args);
        Assert.Contains("TOKEN=a=b\nsecond line\0", config.EnvironmentBlock());
        Assert.EndsWith("\0\0", config.EnvironmentBlock());
    }

    [Theory]
    [InlineData("", "\"\"")]
    [InlineData("a b", "\"a b\"")]
    [InlineData("a\"b", "\"a\\\"b\"")]
    [InlineData("a\\", "\"a\\\\\"")]
    public void QuotesWindowsArguments(string input, string expected) => Assert.Equal(expected, PipeRunConfig.Quote(input));

    [Theory]
    [InlineData("null")]
    [InlineData("{}")]
    [InlineData("{\"executable\":\"node\",\"args\":[],\"cwd\":\"C:\\\\work\",\"env\":{}}")]
    [InlineData("{\"executable\":\"C:\\\\run.cmd\",\"args\":[],\"cwd\":\"C:\\\\work\",\"env\":{}}")]
    [InlineData("{\"executable\":\"C:\\\\run.exe\",\"args\":[],\"cwd\":\"relative\",\"env\":{}}")]
    [InlineData("{\"executable\":\"C:\\\\run.exe\",\"args\":[],\"cwd\":\"C:\\\\work\",\"env\":{},\"extra\":true}")]
    [InlineData("{\"executable\":\"C:\\\\run.exe\",\"args\":[],\"args\":[],\"cwd\":\"C:\\\\work\",\"env\":{}}")]
    [InlineData("{\"executable\":\"C:\\\\run.exe\",\"args\":[],\"cwd\":\"C:\\\\work\",\"env\":{\"PATH\":\"one\",\"path\":\"two\"}}")]
    public void RejectsInvalidConfig(string json) => Assert.ThrowsAny<Exception>(() => PipeRunConfig.Parse(Encoding.UTF8.GetBytes(json)));

    [Fact]
    public void BoundsConfigCommandAndEnvironment()
    {
        Assert.Throws<InvalidDataException>(() => PipeRunConfig.Parse(new byte[PipeRunConfig.MaxBytes + 1]));
        Assert.Throws<InvalidDataException>(() => PipeRunConfig.Parse(Config(new[] { new string('x', 32760) })));
        Assert.Throws<InvalidDataException>(() => PipeRunConfig.Parse(Config(env: new { A = new string('x', 32765) })));
        Assert.Throws<InvalidDataException>(() => PipeRunConfig.Parse(Config(new[] { "nul\0byte" })));
        Assert.Throws<InvalidDataException>(() => PipeRunConfig.Parse(Config(Enumerable.Repeat("", 4097).ToArray())));
    }
}
