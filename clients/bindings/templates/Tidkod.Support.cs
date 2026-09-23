#nullable enable
using System;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;
using Microsoft.Win32.SafeHandles;
using Sys = Tidkod.Sys;
namespace Tidkod;

public enum ConnectionState : byte { Disconnected, Connecting, Connected, Shutdown }
public enum Synchronization : byte { Uninitialized, Acquiring, Synchronized, Holdover }
public enum SourceKind : byte { Generated, Tracked }
public enum SourceHealth : byte { Healthy, Degraded }
public sealed class TidkodException : Exception
{
    public int Status { get; }
    internal TidkodException(int status, string message) : base(message) { Status = status; }
}

// Fail fast on overlapping operations instead of blocking the timecode read path.
internal abstract class OwnedHandle : SafeHandleZeroOrMinusOneIsInvalid
{
    internal int InCall;
    protected OwnedHandle() : base(true) { }
}
// Each call protects lifetime AND exclusive native access. No allocation or mutex.
internal readonly ref struct HandleLease
{
    private readonly OwnedHandle handle;
    internal IntPtr Pointer => handle.DangerousGetHandle();
    internal HandleLease(OwnedHandle value)
    {
        if (Interlocked.CompareExchange(ref value.InCall, 1, 0) != 0)
            throw new InvalidOperationException("Serialize calls on a native handle; use separate readers for separate consumers.");
        bool acquired = false;
        try { value.DangerousAddRef(ref acquired); handle = value; }
        catch { if (acquired) value.DangerousRelease(); Volatile.Write(ref value.InCall, 0); throw; }
    }
    public void Dispose()
    {
        try { handle.DangerousRelease(); }
        finally { Volatile.Write(ref handle.InCall, 0); }
    }
}
internal static unsafe class Native
{
    internal static void Check(int status, Sys.TKBuffer* error)
    {
        if (status == 0) { if (error != null) Sys.NativeMethods.tidkod_buffer_free(error); return; }
        throw new TidkodException(status, error == null ? "Native call failed" : TakeString(error));
    }
    internal static string TakeString(Sys.TKBuffer* value)
    {
        try { return Encoding.UTF8.GetString(new ReadOnlySpan<byte>(Sys.NativeMethods.tidkod_buffer_data(value), checked((int)Sys.NativeMethods.tidkod_buffer_len(value)))); }
        finally { Sys.NativeMethods.tidkod_buffer_free(value); }
    }
    internal static byte[] TakeBytes(Sys.TKBuffer* value)
    {
        try { return new ReadOnlySpan<byte>(Sys.NativeMethods.tidkod_buffer_data(value), checked((int)Sys.NativeMethods.tidkod_buffer_len(value))).ToArray(); }
        finally { Sys.NativeMethods.tidkod_buffer_free(value); }
    }
}

public sealed partial class Core
{
    public Reading ReadForPresentation(ulong nowNs, TimeSpan delay) => ReadForPresentation(nowNs, Durations.Nanoseconds(delay));
}
public sealed partial class TimecodeSnapshot
{
    public Reading ReadForPresentation(ulong nowNs, TimeSpan delay) => ReadForPresentation(nowNs, Durations.Nanoseconds(delay));
}
internal static class Durations
{
    internal static ulong Nanoseconds(TimeSpan value)
    {
        if (value.Ticks < 0) throw new ArgumentOutOfRangeException(nameof(value), "Delay must not be negative");
        return checked((ulong)value.Ticks * 100);
    }
}
