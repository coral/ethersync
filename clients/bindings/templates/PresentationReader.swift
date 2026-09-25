
#if canImport(QuartzCore)
import QuartzCore
import Darwin

/// Own this reader on ONE display/audio execution context. Create a separate
/// native Reader for it; do not share its handle with your session/control actor.
/// Initialization/calibration is off the realtime path. Reads perform no actor hop.
public final class PresentationReader {
    private let reader: Reader
    private var bridge: ClockBridge
    private var timebase = mach_timebase_info_data_t()
    public private(set) var clockUncertaintyNs: Double

    public init(engine: Engine, reader: Reader) throws {
        self.reader = reader
        let calibrated = try Self.calibrate(engine: engine)
        bridge = calibrated.0
        clockUncertaintyNs = calibrated.1
        mach_timebase_info(&timebase)
    }

    /// Recalibrate off the realtime path after system sleep or a clock-domain change.
    public func recalibrate(engine: Engine) throws {
        let calibrated = try Self.calibrate(engine: engine)
        bridge = calibrated.0
        clockUncertaintyNs = calibrated.1
    }

    /// CADisplayLink.targetTimestamp (seconds), not its callback invocation time.
    public func read(atHostTime seconds: Double) throws -> Reading {
        let ns = try Self.nanoseconds(seconds)
        return reader.readAt(nowNs: try bridge.convert(externalNs: ns).localNs)
    }

    /// CoreAudio mHostTime plus only latency NOT already included by the device API.
    /// This maps the buffer's physical output time, not the time the callback ran.
    public func read(atHostTicks ticks: UInt64, additionalLatencyNs: UInt64 = 0) throws -> Reading {
        let whole = ticks / UInt64(timebase.denom)
        let fraction = ticks % UInt64(timebase.denom)
        let product = whole.multipliedReportingOverflow(by: UInt64(timebase.numer))
        let sum = product.partialValue.addingReportingOverflow(fraction * UInt64(timebase.numer) / UInt64(timebase.denom))
        let output = sum.partialValue.addingReportingOverflow(additionalLatencyNs)
        guard !product.overflow, !sum.overflow, !output.overflow else {
            throw TidkodError(description: "Host timestamp overflow")
        }
        return reader.readAt(nowNs: try bridge.convert(externalNs: output.partialValue).localNs)
    }

    /// Capture into an already allocated snapshot for evaluating an audio block.
    public func capture(into snapshot: TimecodeSnapshot) {
        reader.snapshotInto(snapshot: snapshot)
    }
    public func localTime(atHostTime seconds: Double) throws -> OutputTime {
        try bridge.convert(externalNs: Self.nanoseconds(seconds))
    }
    private static func nanoseconds(_ seconds: Double) throws -> UInt64 {
        let ns = seconds * 1e9
        guard ns.isFinite, ns >= 0, ns < Double(UInt64.max) else {
            throw TidkodError(description: "Invalid host timestamp")
        }
        return UInt64(ns)
    }
    private static func calibrate(engine: Engine) throws -> (ClockBridge, Double) {
        var best: (ClockBridge, Double)?
        for _ in 0..<8 {
            let before = engine.now()
            let host = CACurrentMediaTime()
            let after = engine.now()
            let ns = try nanoseconds(host)
            let bridge = try ClockBridge(localBeforeNs: before, externalNs: ns, localAfterNs: after)
            let uncertainty = try bridge.convert(externalNs: ns).uncertaintyNs + max(1, (host * 1e9).ulp)
            if best == nil || uncertainty < best!.1 { best = (bridge, uncertainty) }
        }
        return best!
    }
}
#endif
