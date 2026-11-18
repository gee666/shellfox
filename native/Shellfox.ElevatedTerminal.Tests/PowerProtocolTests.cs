using System.Text;
using System.Text.Json;
using Shellfox.ElevatedTerminal;
using Xunit;

namespace Shellfox.ElevatedTerminal.Tests;

public sealed class PowerProtocolTests
{
    private const string Nonce = "0123456789abcdef0123456789abcdef";
    private static JsonElement Parse(string value) => Wire.Parse(Encoding.UTF8.GetBytes(value));
    private static JsonElement Init(object? changes = null)
    {
        var fields = new Dictionary<string, object?>
        {
            ["type"] = "power-init", ["version"] = 1, ["root"] = @"C:\owned\private-power",
            ["ownerHandle"] = "123", ["ownerBirth"] = "134355986344672889", ["nonce"] = Nonce,
        };
        if (changes != null)
            foreach (var property in JsonSerializer.SerializeToElement(changes).EnumerateObject()) fields[property.Name] = property.Value;
        return Wire.Parse(JsonSerializer.SerializeToUtf8Bytes(fields));
    }
    [Fact]
    public void SafeProbeIsExactTypedV1AndNativeLayoutsMatchWindowsX64AndArm64()
    {
        var probe = Parse(PowerProtocol.Probe);
        Wire.Fields(probe, "protocol", "version", "display", "system", "awayMode", "idleMouse", "lidRollback");
        Assert.Equal("shellfox-power-guardian", probe.GetProperty("protocol").GetString());
        Assert.Equal(1, probe.GetProperty("version").GetInt32());
        foreach (var key in new[] { "display", "system", "awayMode", "idleMouse", "lidRollback" }) Assert.True(probe.GetProperty(key).GetBoolean());
        Assert.Equal((32, 40), PowerNative.Layout()); // Reflection/layout only. No native calls.
    }
    [Fact]
    public void InitCarriesOriginalHandleAndExactBirthWithoutNumericPidFallback()
    {
        var init = PowerProtocol.Init(Init());
        Assert.Equal((nuint)123, init.OwnerHandle); Assert.Equal(134355986344672889L, init.OwnerBirth); Assert.Equal(Nonce, init.Nonce);
    }
    [Theory]
    [InlineData("pid", "123")]
    [InlineData("script", "evil")]
    [InlineData("ownerHandle", "-1")]
    [InlineData("ownerHandle", "0")]
    [InlineData("ownerHandle", "18446744073709551616")]
    [InlineData("ownerBirth", "0")]
    [InlineData("ownerBirth", "1e20")]
    [InlineData("ownerBirth", "18446744073709551615")]
    [InlineData("nonce", "0123456789ABCDEF0123456789ABCDEF")]
    [InlineData("nonce", "bad")]
    [InlineData("root", "relative")]
    [InlineData("root", "\\\\?\\C:\\private")]
    [InlineData("root", "C:\\private\\..\\other")]
    [InlineData("root", "C:\\private:stream")]
    public void InvalidInitFailsBeforeAnyOsSettingAccess(string key, string value)
    {
        var fields = Init().EnumerateObject().ToDictionary(p => p.Name, p => (object)p.Value);
        fields[key] = value;
        Assert.ThrowsAny<Exception>(() => PowerProtocol.Init(Wire.Parse(JsonSerializer.SerializeToUtf8Bytes(fields))));
    }
    [Fact]
    public void UnknownVersionsAndDuplicateFieldsCannotNegotiateOrDowngrade()
    {
        Assert.Throws<InvalidDataException>(() => PowerProtocol.Init(Init(new { version = 2 })));
        var json = Init().GetRawText().Replace("\"type\":", "\"version\":1,\"type\":");
        Assert.Throws<InvalidDataException>(() => PowerProtocol.Init(Parse(json)));
    }
    [Fact]
    public void GoAndStopAreStrictNonceBoundAndNoAutomaticEnableAliasExists()
    {
        Assert.Equal("enable", PowerProtocol.Command(Parse($"{{\"type\":\"power-go\",\"nonce\":\"{Nonce}\",\"action\":\"enable\"}}"), Nonce, true));
        Assert.Equal("recover", PowerProtocol.Command(Parse($"{{\"type\":\"power-go\",\"nonce\":\"{Nonce}\",\"action\":\"recover\"}}"), Nonce, true));
        Assert.Equal("stop", PowerProtocol.Command(Parse($"{{\"type\":\"power-stop\",\"nonce\":\"{Nonce}\"}}"), Nonce, false));
        foreach (var action in new[] { "on", "true", "launch", "", "enable;evil" })
            Assert.Throws<InvalidDataException>(() => PowerProtocol.Command(Parse(JsonSerializer.Serialize(new { type = "power-go", nonce = Nonce, action })), Nonce, true));
        var go = Parse($"{{\"type\":\"power-go\",\"nonce\":\"{Nonce}\",\"action\":\"enable\"}}");
        Assert.Throws<InvalidDataException>(() => PowerProtocol.Command(go, Nonce, false));
        Assert.Throws<InvalidDataException>(() => PowerProtocol.Command(go, new string('a', 32), true));
        Assert.Throws<InvalidDataException>(() => PowerProtocol.Command(Parse($"{{\"type\":\"power-stop\",\"nonce\":\"{Nonce}\",\"pid\":123}}"), Nonce, false));
    }
    [Fact]
    public void NoGoAndOwnerDeathDoesNotPerformEvenStaleRecovery()
    {
        var native = new PowerFakeNative();
        var state = new PowerSnapshot(native.Scheme, 1, 2, true, true, true);
        var journal = new PowerMemoryJournal(state); var controller = new PowerController(new(native, journal));
        controller.Tick(0, false);
        Assert.Equal(PowerPhase.Stopped, controller.Phase); Assert.Empty(native.Calls); Assert.Equal(state, journal.Current);
    }
}
