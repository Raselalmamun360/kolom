<div align="center">

# কলম

**বাংলা ভাষায় লেখা প্রোগ্রামিং ভাষা**

কীওয়ার্ড, টাইপ, ত্রুটি-বার্তা — সবই বাংলায়। অনূদিত কোনো ভাষা নয়, শুরু থেকেই বাংলাভাষীদের জন্য ডিজাইন করা।

[শুরু করুন](docs/getting-started.md) · [টিউটোরিয়াল](docs/tutorial.md) · [স্পেসিফিকেশন](docs/language.md)

</div>

---

```kolom
ইম্পোর্ট লেখা

ফাংশন অভিবাদন(লেখা নাম) -> লেখা {
    ফেরাও "স্বাগতম, " + লেখা.বড়হাতের(নাম)
}

অ্যাপ {
    ধরি নামসমূহ = ["রহিম", "করিম"]

    প্রতি (নাম : নামসমূহ) {
        লেখো(অভিবাদন(নাম))
    }
}
```

```console
$ kolom চালাও অভিবাদন.ক
স্বাগতম, রহিম
স্বাগতম, করিম
```

---

## কেন কলম

- **সম্পূর্ণ বাংলা** — `ধরি`, `যদি`, `ফেরাও`, `লেখো`। ত্রুটি-বার্তাও বাংলায়।
- **নেটিভ কম্পাইলার** — সরাসরি মেশিন কোড। **কোনো C কম্পাইলার, Visual Studio বা Rust ইনস্টল করার দরকার নেই** — কলম নিজের কোড জেনারেটর ও লিংকার নিজেই বহন করে।
- **দ্রুত শেখা** — টাইপ ইনফারেন্স, পরিচিত সিনট্যাক্স। বাংলা (`২৫`) ও ইংরেজি (`25`) দুই ধরনের সংখ্যাই চলে।
- **নিরাপদ মেমরি, GC ছাড়া** — ওনারশিপ ও মুভ মডেল; শেয়ার করা স্টেটের জন্য রেফারেন্স কাউন্টিং।
- **UI ভাষার অংশ** — আলাদা লাইব্রেরি ছাড়াই নেটিভ উইন্ডো, বাটন, ক্যানভাস।

## দ্রুত শুরু

```console
kolom নতুন আমার_প্রকল্প          # নতুন প্রকল্প
kolom চালাও আমার_প্রকল্প/main.ক   # চালাও (ইন্টারপ্রিটেড)
kolom বিল্ড আমার_প্রকল্প/main.ক    # নেটিভ .exe তৈরি করো
```

ইনস্টল ও PATH সেট করার নির্দেশনা — [`docs/getting-started.md`](docs/getting-started.md)।

## একটি জানালা

```kolom
ফাংশন চাপা_হলো() -> ফাঁকা {
    লেখো("বাটন চাপা হয়েছে")
}

অ্যাপ গণক {
    ডিসপ্লে {
        টেক্সট("স্বাগতম")
        সারি() {
            বাটন("চাপুন", চাপা_হলো)
        }
    }
}
```

## অবস্থা

**সংস্করণ ০.১.০** — ভাষা ও কম্পাইলার কাজ করে, তবে এখনো Draft।

| | অবস্থা |
|---|---|
| ইন্টারপ্রেটার (`চালাও`) | ✅ |
| নেটিভ কম্পাইলার (`বিল্ড`) | ✅ Windows x64 |
| স্ট্যান্ডার্ড লাইব্রেরি | ✅ ৮/৯ মডিউল (`নেটওয়ার্ক` শুধু `--সি` ব্যাকএন্ডে) |
| নেটিভ UI ও গ্রাফিক্স | ✅ Windows |
| Linux / macOS নেটিভ বিল্ড | ⏳ পরিকল্পিত (`--সি` ব্যাকএন্ডে আংশিক) |
| IDE এক্সটেনশন | ⏳ পরিকল্পিত |

## কীভাবে কাজ করে

```text
সোর্স (.ক) → লেক্সার → পার্সার → AST → সিমান্টিক অ্যানালাইসিস
                                            ↓
                    ইন্টারপ্রেটার  ←────────┴────────→  Cranelift কোডজেন
                    (কলম চালাও)                          ↓
                                                    অবজেক্ট ফাইল
                                                         ↓
                                              rust-lld (লিংকার)
                                                         ↓
                                                   নেটিভ .exe
```

কোনো ধাপেই C কম্পাইলার লাগে না। **Cranelift** সরাসরি মেশিন কোড তৈরি করে, আর বান্ডলড **rust-lld** সেটিকে এক্সিকিউটেবলে পরিণত করে। বিতরণযোগ্যতার জন্য কলম MinGW-w64 (GNU) ABI টার্গেট করে — কারণ Microsoft-এর `Redist.txt` তাদের `.lib` ফাইল বিতরণের অনুমতি দেয় না।

কম্পাইলারের বিস্তারিত — [`docs/compiler.md`](docs/compiler.md)।

## সোর্স থেকে বিল্ড

প্রয়োজন: Rust টুলচেইন, MinGW-w64।

```console
git clone <repo> && cd Kolom
rustup target add x86_64-pc-windows-gnu
bash scripts/make-sysroot.sh dist release
```

`dist/` ফোল্ডারে সম্পূর্ণ, বিতরণযোগ্য কলম তৈরি হবে।

```console
cargo test --workspace      # সব টেস্ট চালাও
```

## ডকুমেন্টেশন

| ডকুমেন্ট | বিষয় |
|---|---|
| [`getting-started.md`](docs/getting-started.md) | ইনস্টল, প্রথম প্রোগ্রাম, সমস্যা সমাধান |
| [`tutorial.md`](docs/tutorial.md) | ধাপে ধাপে টিউটোরিয়াল |
| [`language.md`](docs/language.md) | ভাষার স্পেসিফিকেশন |
| [`grammer.md`](docs/grammer.md) | ফরমাল গ্রামার (EBNF) |
| [`compiler.md`](docs/compiler.md) | কম্পাইলার আর্কিটেকচার |
| [`engine.md`](docs/engine.md) | UI ও গ্রাফিক্স ইঞ্জিন |
| [`roadmap.md`](docs/roadmap.md) | রোডম্যাপ |

## লাইসেন্স

[MIT](LICENSE-MIT) অথবা [Apache-2.0](LICENSE-APACHE) — যেটি আপনার সুবিধা।

**কলম দিয়ে তৈরি প্রোগ্রামের উপর কোনো লাইসেন্স-বাধ্যবাধকতা নেই** — ক্লোজড সোর্স হিসেবেও বিতরণ করতে পারবেন। বান্ডলড উপাদানের নোটিশ [`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md)-এ।

---

<details>
<summary><b>English summary</b></summary>

<br>

**Kolom** is a programming language written in Bengali — keywords, types, and
error messages are all in Bengali. It is not a translation layer over another
language; it was designed for Bengali speakers from the start.

```kolom
অ্যাপ {
    লেখো("হ্যালো বিশ্ব")
}
```

**Highlights**

- Compiles to native machine code via **Cranelift** — no C compiler, no
  Visual Studio, no Rust needed on the user's machine. The toolchain carries
  its own code generator and linker.
- Statically typed with inference; ownership-based memory management with no
  garbage collector.
- Native UI (windows, buttons, canvas) is part of the language.
- Also runs interpreted for fast iteration.

**Status** — v0.1.0. Interpreter, native compiler, standard library, and UI
all work on Windows x64. Linux/macOS native builds are planned.

**Build from source**

```console
rustup target add x86_64-pc-windows-gnu
bash scripts/make-sysroot.sh dist release
```

**License** — MIT OR Apache-2.0. Programs compiled with Kolom carry no
licensing obligation.

</details>
