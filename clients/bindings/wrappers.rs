//! Generated convenience layers over the ABI bridges, from the same facade AST.
use super::{args, output, suffix};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};
fn snake(name: &str) -> String {
    name.chars()
        .enumerate()
        .flat_map(|(i, c)| {
            let mut s = String::new();
            if c.is_uppercase() && i > 0 {
                s.push('_');
            }
            s.push(c.to_ascii_lowercase());
            s.chars().collect::<Vec<_>>()
        })
        .collect()
}
fn camel(name: &str) -> String {
    let mut parts = name.split('_');
    let mut out = parts.next().unwrap().to_string();
    for p in parts {
        let mut c = p.chars();
        if let Some(first) = c.next() {
            out.extend(first.to_uppercase());
            out.extend(c);
        }
    }
    out
}
fn base(t: &str) -> &str {
    t.trim_start_matches('&').trim_start_matches("mut")
}
fn scalar(t: &str, swift: bool) -> String {
    match (t, swift) {
        ("()", true) => "Void",
        ("()", false) => "void",
        ("bool", true) => "Bool",
        ("bool", false) => "bool",
        ("u8", true) => "UInt8",
        ("u32", true) => "UInt32",
        ("u64", true) => "UInt64",
        ("i32", true) => "Int32",
        ("i64", true) => "Int64",
        ("f64", true) => "Double",
        ("u8", false) => "uint8_t",
        ("u32", false) => "uint32_t",
        ("u64", false) => "uint64_t",
        ("i32", false) => "int32_t",
        ("i64", false) => "int64_t",
        ("f64", false) => "double",
        _ => t,
    }
    .into()
}
pub fn generate(
    dir: &Path,
    opaque: &BTreeSet<String>,
    records: &BTreeMap<String, Vec<(String, String)>>,
    functions: &[syn::ItemFn],
) {
    let owner = |name: &str| {
        opaque
            .iter()
            .filter(|t| name.starts_with(&(snake(t) + "_")))
            .max_by_key(|t| t.len())
            .expect("function needs an owning handle")
            .clone()
    };
    let enums = [
        (
            "Synchronization",
            "synchronization",
            vec!["uninitialized", "acquiring", "synchronized", "holdover"],
        ),
        (
            "ConnectionState",
            "connection",
            vec!["disconnected", "connecting", "connected", "shutdown"],
        ),
        ("SourceKind", "source_kind", vec!["generated", "tracked"]),
        ("SourceHealth", "source_health", vec!["healthy", "degraded"]),
    ];
    if std::env::var_os("CARGO_FEATURE_SWIFT").is_some() {
        let mut s = String::from(
            "// Generated; do not edit.\nimport EthersyncSys\nimport Foundation\npublic struct EthersyncError: Error, CustomStringConvertible { public let description: String }\nprivate func checked<T>(_ body: () throws -> T) throws -> T { do { return try body() } catch let error as RustString { throw EthersyncError(description: error.toString()) } catch { throw error } }\n",
        );
        for (name, _, variants) in &enums {
            s += &format!("public enum {name}: UInt8 {{\n");
            for (i, variant) in variants.iter().enumerate() {
                s += &format!("case `{variant}` = {i}\n");
            }
            s += "case unknown = 255\n}\n";
        }
        for (name, fields) in records {
            s += &format!("public struct {name} {{ fileprivate let raw: EthersyncSys.{name}\n");
            for (n, t) in fields {
                if let Some((kind, _, _)) = enums
                    .iter()
                    .find(|(_, field, _)| name == "Reading" && field == n)
                {
                    s += &format!(
                        "public var `{}`: {kind} {{ {kind}(rawValue: raw.{n}) ?? .unknown }}\n",
                        camel(n)
                    );
                } else {
                    s += &format!(
                        "public var `{}`: {} {{ raw.{n} }}\n",
                        camel(n),
                        scalar(t, true)
                    );
                }
            }
            if name == "Reading" {
                s += "public var timecode: String { String(format: \"%02d:%02d:%02d%@%02d\", hours, minutes, seconds, dropFrame ? \";\" : \":\", frame) }\n";
            }
            s += "}\n";
        }
        for t in opaque {
            let declaration = if t == "Endpoint" {
                "struct"
            } else {
                "final class"
            };
            s += &format!(
                "/// Owned handle. Serialize access; this type is deliberately not Sendable.\npublic {declaration} {t} {{ fileprivate let raw: EthersyncSys.{t}\nfileprivate init(raw: EthersyncSys.{t}) {{ self.raw = raw }}\n"
            );
            for f in functions
                .iter()
                .filter(|f| owner(&f.sig.ident.to_string()) == *t)
            {
                let name = f.sig.ident.to_string();
                let method = name.strip_prefix(&(snake(t) + "_")).unwrap();
                let a = args(f);
                let instance = a
                    .first()
                    .is_some_and(|(_, ty)| ty.starts_with('&') && base(ty) == t);
                let a_rest = &a[usize::from(instance)..];
                let (ret, fallible) = output(f);
                let init = method == "new";
                let typ = |ty: &str| match ty {
                    "String" | "&str" => "String".into(),
                    "Vec<u8>" | "&[u8]" => "[UInt8]".into(),
                    _ => scalar(base(ty), true),
                };
                let decl = a_rest
                    .iter()
                    .map(|(n, ty)| format!("`{}`: {}", camel(n), typ(ty)))
                    .collect::<Vec<_>>()
                    .join(", ");
                s += &if init {
                    format!(
                        "public convenience init({decl}) {} {{\n",
                        if fallible { "throws" } else { "" }
                    )
                } else {
                    format!(
                        "public {}func `{}`({decl}) {} -> {} {{\n",
                        if instance { "" } else { "static " },
                        camel(method),
                        if fallible { "throws" } else { "" },
                        typ(&ret)
                    )
                };
                let callargs = a
                    .iter()
                    .enumerate()
                    .map(|(i, (n, ty))| {
                        if instance && i == 0 {
                            "raw".into()
                        } else if opaque.contains(base(ty)) {
                            format!("`{}`.raw", camel(n))
                        } else if ty == "&[u8]" {
                            "buffer".into()
                        } else {
                            format!("`{}`", camel(n))
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                let mut expr = format!(
                    "{}EthersyncSys.{name}({callargs})",
                    if fallible { "try " } else { "" }
                );
                if let Some((n, _)) = a_rest.iter().find(|(_, ty)| ty == "&[u8]") {
                    expr = format!(
                        "{}`{}`.withUnsafeBufferPointer {{ buffer in {expr} }}",
                        if fallible { "try " } else { "" },
                        camel(n)
                    );
                }
                if fallible {
                    expr = format!("try checked {{ {expr} }}");
                }
                if init {
                    s += &format!("self.init(raw: {expr})\n");
                } else if ret == "()" {
                    s += &format!("{expr}\n");
                } else {
                    s += &format!(
                        "let value = {expr}\nreturn {}\n",
                        if opaque.contains(&ret) || records.contains_key(&ret) {
                            format!("{ret}(raw: value)")
                        } else if ret == "String" {
                            "value.toString()".into()
                        } else if ret == "Vec<u8>" {
                            "Array(value)".into()
                        } else {
                            "value".into()
                        }
                    );
                }
                s += "}\n";
            }
            s += "}\n";
        }
        if opaque.contains("Endpoint") {
            s += include_str!("templates/Endpoint.swift");
        }
        fs::write(dir.join("EthersyncClient.swift"), s).unwrap();
    }
    if std::env::var_os("CARGO_FEATURE_CPP").is_some() {
        let mut s = String::from(
            "// Generated; do not edit.\n#pragma once\n#include \"ethersync.hpp\"\n#include <optional>\n#include <string>\n#include <string_view>\n#include <vector>\n#include <utility>\n#include <cstdlib>\nnamespace ethersync::client {\ntemplate<class T> class Result { std::optional<T> value_; std::string error_; public: explicit Result(T value):value_(std::move(value)){} static Result failure(std::string error){return Result(std::move(error),0);} explicit operator bool() const {return value_.has_value();} const std::string& error() const{return error_;} T& value(){if(!value_)std::abort();return *value_;} T take(){if(!value_)std::abort();T result=std::move(*value_);value_.reset();return result;} private: Result(std::string error,int):error_(std::move(error)){} };\ntemplate<> class Result<void> { bool ok_; std::string error_; public: Result():ok_(true){} static Result failure(std::string error){Result r;r.ok_=false;r.error_=std::move(error);return r;} explicit operator bool() const{return ok_;} const std::string& error() const{return error_;} };\n",
        );
        for t in opaque {
            s += &format!("class {t};\n");
        }
        for t in records.keys() {
            s += &format!("using {t} = ::ethersync::{t};\n");
        }
        let typ = |t: &str| match t {
            "String" => "std::string".into(),
            "&str" => "std::string_view".into(),
            "Vec<u8>" => "std::vector<uint8_t>".into(),
            "&[u8]" => "const std::vector<uint8_t>&".into(),
            _ if t.starts_with('&') => format!("{}&", base(t)),
            _ => scalar(t, false),
        };
        let mut bodies = String::new();
        for t in opaque {
            s += &format!(
                "class {t} {{ rust::Box<::ethersync::{t}> raw_; public:\nexplicit {t}(rust::Box<::ethersync::{t}> raw):raw_(std::move(raw)){{}}\n{t}({t}&&)=default; {t}& operator=({t}&&)=default; {t}(const {t}&)=delete; {t}& operator=(const {t}&)=delete;\n::ethersync::{t}& raw(){{return *raw_;}}\n"
            );
            for f in functions
                .iter()
                .filter(|f| owner(&f.sig.ident.to_string()) == *t)
            {
                let name = f.sig.ident.to_string();
                let method = name.strip_prefix(&(snake(t) + "_")).unwrap();
                let method = if method == "new" { "create" } else { method };
                let a = args(f);
                let instance = a
                    .first()
                    .is_some_and(|(_, ty)| ty.starts_with('&') && base(ty) == t);
                let (ret, fallible) = output(f);
                let rt = if fallible {
                    format!("Result<{}>", typ(&ret))
                } else {
                    typ(&ret)
                };
                let decl = a[usize::from(instance)..]
                    .iter()
                    .map(|(n, t)| format!("{} {n}", typ(t)))
                    .collect::<Vec<_>>()
                    .join(", ");
                s += &format!(
                    "{} {rt} {method}({decl});\n",
                    if instance { "" } else { "static" }
                );
                bodies += &format!("inline {rt} {t}::{method}({decl}) {{\n");
                let callargs = a
                    .iter()
                    .enumerate()
                    .map(|(i, (n, t))| {
                        if instance && i == 0 {
                            "*raw_".into()
                        } else if opaque.contains(base(t)) {
                            format!("{n}.raw()")
                        } else if t == "&str" {
                            format!("rust::Str({n}.data(),{n}.size())")
                        } else if t == "&[u8]" {
                            format!("rust::Slice<const uint8_t>({n}.data(),{n}.size())")
                        } else {
                            n.clone()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                let call = format!("::ethersync::{name}({callargs})");
                if fallible {
                    let outcome = format!("Outcome{}", suffix(&ret));
                    bodies += &format!(
                        "auto result={call}; if(!::ethersync::{outcome}_ok(*result)) return {rt}::failure(std::string(::ethersync::{outcome}_error(*result)));\n"
                    );
                    if ret != "()" {
                        bodies += &format!("auto value=::ethersync::{outcome}_take(*result);\n");
                    }
                } else if ret == "()" {
                    bodies += &format!("{call};\n");
                } else {
                    bodies += &format!("auto value={call};\n");
                }
                if ret != "()" {
                    let converted = if opaque.contains(&ret) {
                        format!("{ret}(std::move(value))")
                    } else if ret == "String" {
                        "std::string(value)".into()
                    } else if ret == "Vec<u8>" {
                        "std::vector<uint8_t>(value.begin(),value.end())".into()
                    } else {
                        "value".into()
                    };
                    bodies += &format!(
                        "return {};\n",
                        if fallible {
                            format!("{rt}({converted})")
                        } else {
                            converted
                        }
                    );
                } else if fallible {
                    bodies += "return Result<void>();\n";
                }
                bodies += "}\n";
            }
            s += "};\n";
        }
        s += &bodies;
        s += "}\n";
        fs::write(dir.join("ethersync-client.hpp"), s).unwrap();
    }
    if std::env::var_os("CARGO_FEATURE_C").is_some() {
        let mut s = String::from(
            "/* Generated; owned structs must not be copied. */\n#ifndef ETHERSYNC_CLIENT_H\n#define ETHERSYNC_CLIENT_H\n#include \"ethersync.h\"\n#include <string.h>\n",
        );
        for t in opaque {
            s += &format!(
                "typedef struct {{ {t} *raw; }} Es{t};\nstatic inline void es_{}_dispose(Es{t} *v) {{ ethersync_{}_free(v->raw); v->raw = NULL; }}\n",
                snake(t),
                t.to_lowercase()
            );
        }
        s += "typedef struct { const uint8_t *data; size_t len; } EsBytes;\nstatic inline void es_buffer_dispose(EsBuffer **value) { ethersync_buffer_free(*value); *value = NULL; }\nstatic inline EsBytes es_buffer_view(const EsBuffer *value) { EsBytes bytes = {ethersync_buffer_data(value), ethersync_buffer_len(value)}; return bytes; }\n";
        let ty = |t: &str| {
            if opaque.contains(t) {
                format!("Es{t}")
            } else if t == "String" || t == "Vec<u8>" {
                "EsBuffer *".into()
            } else {
                scalar(t, false)
            }
        };
        let returns: BTreeSet<_> = functions.iter().map(|f| output(f).0).collect();
        for t in returns {
            s += &format!(
                "typedef struct {{ int32_t status; EsBuffer *error; {} }} EsResult{};\n",
                if t == "()" {
                    String::new()
                } else {
                    format!("{} value;", ty(&t))
                },
                suffix(&t)
            );
        }
        for f in functions {
            let name = f.sig.ident.to_string();
            let a = args(f);
            let (ret, _) = output(f);
            let result = format!("EsResult{}", suffix(&ret));
            let decl = a
                .iter()
                .map(|(n, t)| {
                    format!(
                        "{} {n}",
                        if t == "&str" {
                            "const char *".into()
                        } else if t == "&[u8]" {
                            "EsBytes".into()
                        } else {
                            ty(base(t))
                        }
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            let mut callargs = a
                .iter()
                .map(|(n, t)| {
                    if t == "&str" {
                        format!("(const uint8_t*){n}, {n} ? strlen({n}) : 0")
                    } else if t == "&[u8]" {
                        format!("{n}.data, {n}.len")
                    } else if opaque.contains(base(t)) {
                        format!("{n}.raw")
                    } else {
                        n.clone()
                    }
                })
                .collect::<Vec<_>>();
            if ret != "()" {
                callargs.push(if opaque.contains(&ret) {
                    "&result.value.raw".into()
                } else {
                    "&result.value".into()
                });
            }
            callargs.push("&result.error".into());
            s += &format!(
                "static inline {result} es_{name}({}) {{ {result} result = {{0}}; result.status = ethersync_{name}({}); return result; }}\n",
                if decl.is_empty() { "void" } else { &decl },
                callargs.join(", ")
            );
        }
        s += "#endif\n";
        fs::write(dir.join("ethersync-client.h"), s).unwrap();
    }
}
