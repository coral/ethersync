# Protocol Documentation
<a name="top"></a>

## Table of Contents

- [tidkod/v1/tidkod.proto](#tidkod_v1_tidkod-proto)
    - [Anchor](#tidkod-v1-Anchor)
    - [FrameFormat](#tidkod-v1-FrameFormat)
    - [Position](#tidkod-v1-Position)
    - [Probe](#tidkod-v1-Probe)
    - [Rate](#tidkod-v1-Rate)
    - [ScheduledChange](#tidkod-v1-ScheduledChange)
    - [Snapshot](#tidkod-v1-Snapshot)

    - [SourceHealth](#tidkod-v1-SourceHealth)
    - [SourceKind](#tidkod-v1-SourceKind)

- [Scalar Value Types](#scalar-value-types)



<a name="tidkod_v1_tidkod-proto"></a>
<p align="right"><a href="#top">Top</a></p>

## tidkod/v1/tidkod.proto



<a name="tidkod-v1-Anchor"></a>

### Anchor
A complete trajectory, timestamped in leader monotonic nanoseconds.


| Field | Type | Label | Description |
| ----- | ---- | ----- | ----------- |
| time_ns | [uint64](#uint64) |  |  |
| position | [Position](#tidkod-v1-Position) |  |  |
| rate | [Rate](#tidkod-v1-Rate) |  |  |






<a name="tidkod-v1-FrameFormat"></a>

### FrameFormat
Supported exact frame rate and display convention.


| Field | Type | Label | Description |
| ----- | ---- | ----- | ----------- |
| numerator | [uint32](#uint32) |  |  |
| denominator | [uint32](#uint32) |  |  |
| drop_frame | [bool](#bool) |  |  |






<a name="tidkod-v1-Position"></a>

### Position
Signed unwrapped frames plus a nonnegative fraction in units of 2^-32 frames.


| Field | Type | Label | Description |
| ----- | ---- | ----- | ----------- |
| frames | [sint64](#sint64) |  |  |
| subframe | [fixed32](#fixed32) |  |  |






<a name="tidkod-v1-Probe"></a>

### Probe
Private unreliable request/reply; response echoes sequence and t1.


| Field | Type | Label | Description |
| ----- | ---- | ----- | ----------- |
| version | [uint32](#uint32) |  |  |
| sequence | [uint64](#uint64) |  |  |
| t1 | [uint64](#uint64) |  |  |
| t2 | [uint64](#uint64) |  |  |
| t3 | [uint64](#uint64) |  |  |






<a name="tidkod-v1-Rate"></a>

### Rate
Signed rational playback multiplier; zero means paused.


| Field | Type | Label | Description |
| ----- | ---- | ----- | ----------- |
| numerator | [sint32](#sint32) |  |  |
| denominator | [uint32](#uint32) |  |  |






<a name="tidkod-v1-ScheduledChange"></a>

### ScheduledChange
A retained future trajectory, applied exactly at anchor.time_ns.


| Field | Type | Label | Description |
| ----- | ---- | ----- | ----------- |
| discontinuity | [uint64](#uint64) |  |  |
| anchor | [Anchor](#tidkod-v1-Anchor) |  |  |






<a name="tidkod-v1-Snapshot"></a>

### Snapshot
One complete independent state group. Protocol v1 has no delta messages.


| Field | Type | Label | Description |
| ----- | ---- | ----- | ----------- |
| version | [uint32](#uint32) |  |  |
| session | [bytes](#bytes) |  |  |
| revision | [uint64](#uint64) |  |  |
| discontinuity | [uint64](#uint64) |  |  |
| source_kind | [SourceKind](#tidkod-v1-SourceKind) |  |  |
| source_health | [SourceHealth](#tidkod-v1-SourceHealth) |  |  |
| format | [FrameFormat](#tidkod-v1-FrameFormat) |  |  |
| anchor | [Anchor](#tidkod-v1-Anchor) |  |  |
| scheduled | [ScheduledChange](#tidkod-v1-ScheduledChange) | repeated |  |








<a name="tidkod-v1-SourceHealth"></a>

### SourceHealth
Health of the timecode source, independent of connection health.

| Name | Number | Description |
| ---- | ------ | ----------- |
| SOURCE_HEALTH_UNSPECIFIED | 0 |  |
| SOURCE_HEALTH_HEALTHY | 1 |  |
| SOURCE_HEALTH_DEGRADED | 2 |  |



<a name="tidkod-v1-SourceKind"></a>

### SourceKind
Source provenance does not change when external input disappears.

| Name | Number | Description |
| ---- | ------ | ----------- |
| SOURCE_KIND_UNSPECIFIED | 0 |  |
| SOURCE_KIND_GENERATED | 1 |  |
| SOURCE_KIND_TRACKED | 2 |  |










## Scalar Value Types

| .proto Type | Notes | C++ | Java | Python | Go | C# | PHP | Ruby |
| ----------- | ----- | --- | ---- | ------ | -- | -- | --- | ---- |
| <a name="double" /> double |  | double | double | float | float64 | double | float | Float |
| <a name="float" /> float |  | float | float | float | float32 | float | float | Float |
| <a name="int32" /> int32 | Uses variable-length encoding. Inefficient for encoding negative numbers – if your field is likely to have negative values, use sint32 instead. | int32 | int | int | int32 | int | integer | Bignum or Fixnum (as required) |
| <a name="int64" /> int64 | Uses variable-length encoding. Inefficient for encoding negative numbers – if your field is likely to have negative values, use sint64 instead. | int64 | long | int/long | int64 | long | integer/string | Bignum |
| <a name="uint32" /> uint32 | Uses variable-length encoding. | uint32 | int | int/long | uint32 | uint | integer | Bignum or Fixnum (as required) |
| <a name="uint64" /> uint64 | Uses variable-length encoding. | uint64 | long | int/long | uint64 | ulong | integer/string | Bignum or Fixnum (as required) |
| <a name="sint32" /> sint32 | Uses variable-length encoding. Signed int value. These more efficiently encode negative numbers than regular int32s. | int32 | int | int | int32 | int | integer | Bignum or Fixnum (as required) |
| <a name="sint64" /> sint64 | Uses variable-length encoding. Signed int value. These more efficiently encode negative numbers than regular int64s. | int64 | long | int/long | int64 | long | integer/string | Bignum |
| <a name="fixed32" /> fixed32 | Always four bytes. More efficient than uint32 if values are often greater than 2^28. | uint32 | int | int | uint32 | uint | integer | Bignum or Fixnum (as required) |
| <a name="fixed64" /> fixed64 | Always eight bytes. More efficient than uint64 if values are often greater than 2^56. | uint64 | long | int/long | uint64 | ulong | integer/string | Bignum |
| <a name="sfixed32" /> sfixed32 | Always four bytes. | int32 | int | int | int32 | int | integer | Bignum or Fixnum (as required) |
| <a name="sfixed64" /> sfixed64 | Always eight bytes. | int64 | long | int/long | int64 | long | integer/string | Bignum |
| <a name="bool" /> bool |  | bool | boolean | boolean | bool | bool | boolean | TrueClass/FalseClass |
| <a name="string" /> string | A string must always contain UTF-8 encoded or 7-bit ASCII text. | string | String | str/unicode | string | string | string | String (UTF-8) |
| <a name="bytes" /> bytes | May contain any arbitrary sequence of bytes. | string | ByteString | str | []byte | ByteString | string | String (ASCII-8BIT) |
