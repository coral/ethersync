#nullable enable
using System;
using System.Net;
using System.Net.Sockets;
using Sys = Tidkod.Sys;
namespace Tidkod;

public sealed partial class Endpoint
{
    public static Endpoint From(IPEndPoint endpoint)
    {
        ArgumentNullException.ThrowIfNull(endpoint);
        return endpoint.AddressFamily switch {
            AddressFamily.InterNetwork => Ipv4(endpoint.Address.GetAddressBytes(), checked((uint)endpoint.Port)),
            AddressFamily.InterNetworkV6 => Ipv6(endpoint.Address.GetAddressBytes(), checked((uint)endpoint.Port), checked((uint)endpoint.Address.ScopeId)),
            _ => throw new ArgumentException("Only numeric IPv4 and IPv6 endpoints are supported", nameof(endpoint))
        };
    }
    public IPEndPoint ToIPEndPoint() => IPEndPoint.Parse(Address());
}
public sealed partial class LeaderOptions
{
    public void Bind(IPEndPoint endpoint) { using var value = Endpoint.From(endpoint); Bind(value); }
}
public sealed unsafe partial class FollowerOptions
{
    public void Bind(IPEndPoint endpoint) { using var value = Endpoint.From(endpoint); Bind(value); }
    public FollowerOptions(IPEndPoint endpoint) : this(Create(endpoint)) { }
    private static unsafe Sys.FollowerOptions* Create(IPEndPoint endpoint)
    {
        using var value = Endpoint.From(endpoint);
        using var lease = new HandleLease(value.Handle);
        Sys.FollowerOptions* result = null;
        Sys.TKBuffer* error = null;
        Native.Check(Sys.NativeMethods.tidkod_follower_options_endpoint((Sys.Endpoint*)lease.Pointer, &result, &error), error);
        return result;
    }
}

public sealed partial class Reader
{
    public Reading ReadForPresentation(ulong nowNs, TimeSpan delay) => ReadForPresentation(nowNs, Durations.Nanoseconds(delay));
}
