# Snapdeck Recording Stage 2 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ayarlarda `Record system audio` açıkken bir kayıt, Mac'in çaldığı sesi (Snapdeck'in kendi sesi hariç) aynı MP4'ün içine alır ve yeni bir izin istenmez. Mikrofon, yalnız ölçüm kapısı "oynatılabilir MP4, ses izi var" derse ve CTO bunu onaylarsa gelir: `Record the microphone` açıkken ilk kayıtta macOS izin sorar; izin varsa mikrofon aynı dosyaya girer, yoksa kayıt mikrofonsuz başlar ve bunu söyler.

**Architecture:** Kodlayıcı yine yazılmaz. Ses, Aşama 1'in `SCStream`'ine yapılandırma bayraklarıyla eklenir ve `SCRecordingOutput` sesi aynı dosyaya yazar (tasarım 3.2, 5.4). **Ses için output handler takılmaz:** `screencapturekit` 9.0.1'in `remove_recording_output`'u filmin sonlanmasını ancak stream'de hiç handler kalmamışsa bekler (`src/stream/sc_stream.rs:996-998`); Aşama 1'in durdurma sırası bu planda tek satır değişmez. Mikrofon izni kaydın başında, akış kurulmadan önce sorulur; karar saf bir fonksiyondadır ve istek fonksiyonu enjekte edilir.

**Tech Stack:** Rust (workspace), `screencapturekit` 9.0.1 (`macos_15_0`), Tauri 2.11.5, TypeScript (strict), Vitest. 2b'de ek olarak `objc2` 0.6.4 ve `block2` 0.6.2; ikisi de `Cargo.lock`'ta zaten var.

---

## Global Constraints

- Kayıt macOS 15.0+ (Aşama 1'le aynı kapı). Sistem sesi API'si tabandadır, mikrofon API'si `macos_15_0` arkasındadır ve özellik zaten açık; **yeni sürüm kapısı yok**.
- Kod, yorum, commit mesajı ve arayüz metinleri İngilizce; yalnız `docs/superpowers/` Türkçe.
- Ağ çağrısı yok, telemetri yok.
- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `pnpm lint`, `pnpm test`, `pnpm build` her commit'te temiz.
- **Yeni paket yok.** `Cargo.lock`'a yeni `[[package]]` girişi girmez. 2a'nın sonunda `Cargo.lock` hiç değişmemiştir. 2b'de tek değişiklik, `snapdeck-capture` girişinin `dependencies` listesine mevcut `"block2"` ve `"objc2"` satırlarının eklenmesidir.
- **Durdurma sırası dokunulmaz.** `tear_down`, `release`, `RecordingTeardown`, `FrameCounter`, `FrameCounts`, `failure_to_report`, `impl Drop for MacRecording` ve C1-C4, C9, C10, D1, D2 testleri tek satır değişmez. `SCStreamOutputType::Audio` ve `SCStreamOutputType::Microphone` üretim kodunda hiç geçmez (Görev 1'in AU5 testi çiviler).
- **Dosya yolu değişmez.** Geçici dosya, yer tutucu, rename, süpürme, `finish`, `discard`, tepsi ve menü Aşama 1'deki gibi kalır. Yeni Tauri komutu yok, capability değişikliği yok, tepsi değişikliği yok.
- `packages/editor`, `apps/extension`, `crates/stitch` bu planda hiç açılmaz.
- **Mikrofon kapısı gerçek bir kapıdır.** Görev 6, 7 ve 8, bu dosyanın "Mikrofon ölçüm kapısı raporu" bölümünde `Sonuç: GEÇTİ` ve tarihli bir `CTO onayı` satırı yoksa **başlamaz**; ajan hiçbir dosyaya dokunmadan döner ve bunu raporlar. Onayı hiçbir uygulayıcı ajan kendi raporu için yazamaz; CTO (orkestratör) kullanıcıyla teyit edip yazar.
- **Donanım ölçümü kuralları** (Aşama 1 Task 9'un dersleri): her ölçüm önce kaydın gerçekten başladığını kanıtlar; kayıtlar kullanıcının canlı ekranını ve mikrofonunu içerir, **hiçbiri açılıp izlenmez veya dinlenmez**, yalnız sayıları okunur ve hemen silinir; TCC istemlerini kullanıcı onaylar, ajan onaylayamaz; ad-hoc imzalı her yeni derleme ekran ve mikrofon izinlerini sıfırlar.

## Bu planın kapsamı dışında

Tasarım 3.2'nin kapsam dışı bıraktıkları ve bu planın almadıkları, birebir:

- Ses seviyesi göstergesi, giriş cihazı seçimi (`with_microphone_capture_device_id`, audio.rs:446, **kullanılmaz**), gürültü azaltma.
- Sesin ayrı dosyaya yazılması, sistem sesi ile mikrofonun ayrı seviyelerde miksajı, örnekleme hızı veya kanal sayısı ayarı.
- Mikrofon kötü ölçülürse ayrı AAC + mux yolu (tasarım 16, S3: girilmez).
- GIF ve trim (Aşama 3), kayıt kısayolu, editör, panoya kopyalama.
- Mikrofon izni için tıklanabilir bir derin bağlantı yüzeyi. Şikâyet metni ayar yolunu adıyla söyler; tepsi ipucu tıklanamaz (bkz. Açık sorular, S8).
- Tepsi ve menüde yeni öğe veya yeni uyarı yüzeyi.

## Açık sorular (CTO)

Plan her biri için **en dar yorumu** uyguluyor; farklı bir cevap planı değiştirir.

- **S1.** İki ayarın varsayılanı. Tasarım söylemiyor. Plan: ikisi de `false` (Aşama 1'in sessiz filmi korunur; mikrofon için v1 kuralı zaten bunu ister).
- **S2.** Mikrofon reddedildiğinde şikâyet `report_failure` ile verilir. Aşama 1'in yürürlükteki kuralı "hata tepsi başlığını kazanır" olduğundan şikâyet görünürken geçen süre sayacı gizlenir. Tasarım 9.2 ise "kayıt sürerken başlık sayacındır" diyor. Plan Aşama 1 kodunu izler; diğer yol yeni bir uyarı yüzeyi demektir ve bu planda yok.
- **S3.** CHANGELOG bölümü: `v0.3.0` henüz etiketlenmediyse ses oraya mı yazılsın? Plan: etiket yoksa `v0.3.0`'a yazılır, varsa ajan durup sorar.
- **S4.** Ölçüm betiği (`probe.swift`) depoya girsin mi? Plan: girmez, scratch'te yaşar; Görev 4, 5 ve 9 aynı çıktı sözleşmesiyle yazar.
- **S5.** Kapı M3'te (sistem + mikrofon) iki ayrı ses izi raporlarsa kabul edilecek mi? Plan: bu karar değil bayraktır; CTO onay satırında açıkça yazar.
- **S6.** macOS 15.x makine yoksa kapı yalnız mevcut makinenin sürümünde ölçülür. Plan: onay satırı bunu açıkça kabul etmeden 2b başlamaz.
- **S7.** Ayar penceresindeki `Recording` bölümü macOS 14'te gizlensin mi? Plan: gizlenmez; README kaydın macOS 15 istediğini zaten söylüyor.
- **S8.** `Privacy_Microphone` derin bağlantısı (tasarım 8.3) bir yüzeye bağlansın mı? Plan: bağlanmaz, metin yolu adıyla söyler.

---

## Tasarımdan sapmalar

Uygulayıcı bunlardan biriyle çelişen bir şey bulursa **uygulamaz, rapor eder**.

| # | Tasarım | Bu plan | Gerekçe |
|---|---|---|---|
| Sapma 1 | 6.1 ve 6.2: `record(target, &RecordingOptions, destination)`, `RecordingOptions { fps, shows_cursor, audio }` | `record(target, audio: AudioSources, output)`; `RecordingOptions` yok | Aşama 1 fps'i ve imleci sabit yaptı ve `options`'ı almadı. Tek alanlı bir sarmalayıcı erken soyutlama olurdu. |
| Sapma 2 | 10.2: `apps/desktop/src-tauri/Info.plist` **yeni** | Dosya zaten var (`LSUIElement`); anahtar bu dosyaya eklenir | Diskte ölçüldü. Uygulamanın dock'ta görünmemesi, tauri-bundler'ın bu dosyayı zaten birleştirdiğinin kanıtıdır. |
| Sapma 3 | 8.3: istem "ilk kaydı başlattığında" çıkar | İstem, kayıt başlarken `+[AVCaptureDevice requestAccessForMediaType:completionHandler:]` ile **açıkça** istenir | ScreenCaptureKit'in örtük istem davranışı ölçülmedi ve belgelenmedi. Açık istek olmadan ilk kayıttaki ret kullanıcıya söylenemez; tasarım 8.3 ve 11 bunu şart koşuyor. Crate'te izin API'si yok (`AudioDevices.swift` yalnız cihaz listeliyor), bu yüzden `objc2` + `block2` kenarı gerekiyor. |
| Sapma 4 | Tasarım imza yetkisinden söz etmiyor | Görev 8 önce `codesign` ile hardened runtime'ı ölçer; varsa `com.apple.security.device.audio-input` yetkisi eklenir | `tauri-utils` 2.9.3 `config.rs:655-656`: `hardened_runtime` varsayılanı `true`. Hardened runtime altında bu yetki olmadan mikrofon erişimi reddedilir. Ad-hoc imzada bayrağın uygulanıp uygulanmadığı kaynaktan doğrulanamadı (tauri-bundler kaynağı yerelde yok), bu yüzden ölçülür. |
| Sapma 5 | 5.4: forum 805892'nin kök nedeni biçim açıklamalarının karışması | Kapı aynen kalır | Forum yeniden okundu: soran kişi gerçekten `SCRecordingOutput` kullanıyor (HEVC ile). Apple mühendisinin yanıtı bu yolu doğrudan ele almıyor ve sayfada macOS sürümü yok. Risk ne doğrulandı ne çürütüldü; ölçüm karar verir. |
| Sapma 6 | 12.2: "Snapdeck'in kendi sesi kaydedilmiyor" donanımda doğrulanır | Donanımda gözlenemez; yapılandırma birim testiyle (AU1) kanıtlanır | Uygulama kaynağında ses çalan kod yok (`NSSound` veya benzeri bulunmadı). Koşulmayan bir ölçüm koşulmuş gibi yazılmaz. |

---

## Plan formatı hakkında

Tip tanımları, fonksiyon imzaları ve test durumları **birebir** verilir: sözleşme budur, uygulayıcı değiştiremez. Gövdeyi uygulayıcı yazar. Her test için **mutasyon kanıtı** verilir; testi yazan kişi o satırı gerçekten bozup kırmızıyı görmekle yükümlüdür. TDD zorunludur: önce test, sonra kırmızı, sonra kod.

### Mutasyon tuzağı (Aşama 2 örnekleri)

**İddiayı, test ettiğin değerden türetme. Sözleşme değerini teste açık yaz.**

Yanlış:

```rust
let audio = AudioSources { system: true };
assert_eq!(recording_config(800, 600, None, audio).captures_audio(), audio.system);
```

Bu test, yapılandırma `audio.system`'i okumasa ve `true` sabitlese de yeşildir. Doğrusu iki ayrı iddiadır: `AudioSources { system: true }` için `captures_audio() == true` ve `AudioSources { system: false }` için `captures_audio() == false`, ikisi de literal.

Aynı kural bu planın her yerinde geçerli:

- `microphone_status_from_raw` beklentileri `0`, `1`, `2`, `3` diye yazılır ve değerler **Apple SDK başlığından** okunur, aynı dosyada tanımlanmış bir Rust sabitinden değil.
- Şikâyet metni iddiaları `"without the microphone"` ve `"Privacy & Security > Microphone"` diye literal yazılır, `RECORDING_WITHOUT_MICROPHONE` sabitinden türetilmez.
- Ayar penceresi kablolaması (`recordSystemAudio`) Rust testinde **TypeScript kaynağından** okunur: beklenti karşı taraftan gelir.
- C12 ve C13'teki ses izi sayısı elle kurulmuş MP4 kutularıyla sınanmış bir yardımcıdan gelir (HD1). Beklenen sayı kapı raporundan literal yazılır.
- **Ölçümün de mutasyon kanıtı vardır:** ölçüm betiği, yarıya kesilmiş bir kopyada "oynatılamaz" demiyorsa betik geçersizdir ve onunla alınan hiçbir sonuç sayılmaz.

---

## Dosya sahipliği ve paralellik kuralı

**Aynı sahiplik biriminde aynı anda yalnız bir ajan çalışır.**

| Birim | Kapsam |
|---|---|
| U1 | `crates/frame/**` |
| U2 | `crates/capture/**` |
| U3 | `apps/desktop/src-tauri/**` (`Info.plist`, `tauri.conf.json` dahil) |
| U4 | `apps/desktop/src/**` |
| U5 | `README.md`, `CHANGELOG.md` |
| U6 | Bu plan dosyasının "Uygulama sırasında alınan ek kararlar" bölümü |
| D | Donanım: ekran, hoparlör, mikrofon, TCC istemleri. Aynı anda tek ölçüm, çünkü iki kayıt birbirinin sesini ve ekranını alır. |

### Görev sırası ve paralellik

```
Görev 1 (U1+U2, U3'te tek çağrı) ---+---> Görev 3 (U3) ------------+
Görev 2 (U4) -----------------------+                              +---> Görev 5 (U5+D)
Görev 1 ---------------------------------> Görev 4 (D+U6, KAPI) ---+
                                                  |
                                   KAPI: GEÇTİ + CTO onayı
                                                  |
                      +---------------------------+---------------------------+
                      v                                                       v
       Görev 6 (U1+U2, U3'te derleme)                                  Görev 7 (U4)
                      +---------------------------+---------------------------+
                                                  v
                                           Görev 8 (U3) ---> Görev 9 (U5+D, Görev 5'ten sonra)
```

- **Paralel grup A:** Görev 1 ve Görev 2. Görev 1 trait imzasını değiştirdiği için U1, U2 ve U3'ü birlikte kilitler.
- **Paralel grup B:** Görev 3 (U3) ve Görev 4. Görev 4 ayrı bir `git worktree`'de koşar ve D'yi alır; ana çalışma ağacına dokunmaz.
- **Görev 5:** Görev 3 bitti ve Görev 4 D'yi bıraktı.
- **KAPI.** `GEÇTİ` ve onay yoksa grup C başlamaz.
- **Paralel grup C:** Görev 6 ve Görev 7. Görev 6, U3'te yalnız derleme için zorunlu literal güncellemelerini yapar; grup C'de U3'ün başka sahibi yok.
- **Görev 8** tek başına. **Görev 9** en sonda (U5 ve D, Görev 5'ten sonra).
- **KALDI yolu:** Görev 6, 7 ve 8 iptal olur. Görev 9 yalnız belge görevi olarak koşar (donanım yok).

---

## File Structure

```
crates/frame/src/types.rs                         AudioSources (2a: system, 2b: microphone)
crates/frame/src/lib.rs                           re-export listesi (ekleme)

crates/capture/src/lib.rs                         record(target, audio, output), re-export
crates/capture/src/macos/mod.rs                   record imzası, recording_config'e audio
crates/capture/src/macos/recording.rs             recording_config ses bayrakları, AU/HD testleri, C12, C13
crates/capture/src/macos/permission.rs            2b: microphone_permission, request_microphone_permission
crates/capture/Cargo.toml                         2b: objc2, block2 (macOS hedefi)

apps/desktop/src-tauri/src/settings.rs            record_system_audio, record_microphone
apps/desktop/src-tauri/src/recording.rs           audio_sources, microphone_grant, start'ta kablolama
apps/desktop/src-tauri/src/commands.rs            yalnız testler (settings_to_store)
apps/desktop/src-tauri/Info.plist                 2b: NSMicrophoneUsageDescription
apps/desktop/src-tauri/Entitlements.plist         2b, KOŞULLU: com.apple.security.device.audio-input
apps/desktop/src-tauri/tauri.conf.json            2b, KOŞULLU: bundle.macOS.entitlements

apps/desktop/src/settings/SettingsWindow.tsx      Recording bölümü, iki onay kutusu

README.md, CHANGELOG.md                           ses belgeleri
```

## Yeni bağımlılıklar

**2a: yok.** `Cargo.lock` değişmez.

**2b** (yalnız kapı GEÇTİ ise), `crates/capture/Cargo.toml` içinde `[target.'cfg(target_os = "macos")'.dependencies]`:

| Satır | Neden |
|---|---|
| `objc2 = { version = "0.6", default-features = false, features = ["std"] }` | `+[AVCaptureDevice authorizationStatusForMediaType:]` çağrısı. Satır, `apps/desktop/src-tauri/Cargo.toml:101`'deki mevcut tanımın birebir aynısı. Kilitli sürüm 0.6.4; `class!` (`src/macros/mod.rs:75`), `msg_send!` (`:1246`), `runtime::Bool` (`src/runtime/mod.rs:55`) kaynaktan doğrulandı. |
| `block2 = { version = "0.6", default-features = false, features = ["std"] }` | `requestAccessForMediaType:completionHandler:`'ın tamamlama bloğu. Kilitli sürüm 0.6.2 (`dispatch2` üzerinden zaten ağaçta); `RcBlock::new` `src/rc_block.rs:143`, `std` özelliği `Cargo.toml:85`. |

**Eklenmeyenler:** `objc2-av-foundation` (ağaçta yok ve iki çağrı için bağlama crate'i gerekmez), ses için ayrı yazıcı ya da mux (S3).

---

### Task 1: Ses kaynağı tipi ve sistem sesinin akışa bağlanması

**Birim:** U1 + U2 (+ U3'te tek çağrı satırı). **Bağımlılık:** yok.

> **Birim kilidi:** trait imzası değiştiği için derleme üç birimde birden kırılır ve aynı commit'te düzelmelidir. Bu görev sürerken U1, U2 ve U3 kilitlidir; yalnız Görev 2 (U4) paralel koşar.

**Files:** `crates/frame/src/{types.rs,lib.rs}`, `crates/capture/src/lib.rs`, `crates/capture/src/macos/{mod.rs,recording.rs}`, `apps/desktop/src-tauri/src/recording.rs` (yalnız `start` içindeki `.record(` çağrısı)

**Kaynaktan doğrulanan API** (`screencapturekit-9.0.1/src/stream/configuration/audio.rs`): `with_captures_audio(bool)` :190, `captures_audio()` :196, `with_excludes_current_process_audio(bool)` :383, `excludes_current_process_audio()` :389. Üçü de özellik bayrağı istemez.

**Interfaces (birebir):**

```rust
// crates/frame/src/types.rs

/// Which sound a recording takes in besides the picture.
///
/// `Default` is silence, which is what every recording was before sound
/// existed: a caller that says nothing about sound gets the Stage 1 movie.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AudioSources {
    /// What the Mac is playing, from every process but this one.
    pub system: bool,
}
```

```rust
// crates/frame/src/lib.rs ve crates/capture/src/lib.rs re-export listelerine `AudioSources` eklenir.

// crates/capture/src/lib.rs, ScreenCapturer
    fn record(
        &self,
        target: CaptureTarget,
        audio: AudioSources,
        output: &Path,
    ) -> Result<Box<dyn Recording>, CaptureError> {
        let _ = (target, audio, output);
        Err(CaptureError::Unsupported(
            "this platform cannot record the screen".to_string(),
        ))
    }
```

```rust
// crates/capture/src/macos/recording.rs

/// The stream configuration a recording runs with.
/// (mevcut doc yorumu korunur; sesin ne yaptığını anlatan paragraf eklenir)
pub(crate) fn recording_config(
    width: u32,
    height: u32,
    source_rect: Option<CGRect>,
    audio: AudioSources,
) -> SCStreamConfiguration;
```

**Kurallar:**

- `recording_config`'in mevcut zincirine iki çağrı eklenir: `.with_captures_audio(audio.system)` ve `.with_excludes_current_process_audio(true)`. İkincisi koşulsuzdur: sistem sesi kapalıyken etkisizdir, açıkken Snapdeck'in kendi sesini dışarıda bırakır (tasarım 8.2).
- `with_sample_rate` ve `with_channel_count` **çağrılmaz**. Ayar yok; crate'in varsayılanı (48000 Hz, stereo, audio.rs:9-19) kalır.
- **Ses için output handler eklenmez.** Gerekçe sc_stream.rs:996-998; bkz. Global Constraints.
- `mod.rs`'teki `record`'da yalnız imza ve `recording::recording_config(` çağrısının argümanları değişir; 1-7 numaralı adımlar ve yorumları değişmez.
- C6 ve C7 çağrılarına yalnız `AudioSources::default()` argümanı eklenir, iddiaları değişmez. C11'deki çağrı `.record(CaptureTarget::Display(..), AudioSources::default(), &path)` olur.
- `mock.rs` tek satır değişmez.
- Uygulamada `recording::start`'ın 7. adımı bu görevde `AudioSources::default()` geçer. Ayarı okumak Görev 3'ün işidir; davranış Aşama 1'le aynı kalır.
- `audio` parametresini anlatan doc satırı `fn record`'un hemen üstüne eklenir. Mevcut doc yorumları yerinden oynatılmaz (bkz. Öneriler, plan dışı).

- [ ] **Step 1: Testleri yaz**

`crates/capture/src/macos/recording.rs` testleri:

| Test | Ne kanıtlar | Mutasyon kanıtı |
|---|---|---|
| AU1 `recording_config(800, 600, None, AudioSources { system: true })`: `captures_audio() == true`, `excludes_current_process_audio() == true`, `width() == 800`, `height() == 600`, `fps() == 30`, `shows_cursor() == true` (hepsi literal) | sistem sesi akışa bağlı, ses eklenirken görüntü kararları bozulmadı | `with_captures_audio(false)` sabitle: kırmızı. `with_excludes_current_process_audio` satırını sil: kırmızı |
| AU2 `recording_config(800, 600, None, AudioSources { system: false })`: `captures_audio() == false`, `excludes_current_process_audio() == true` | sessiz istenen kayıt sessiz kalır | `with_captures_audio(true)` sabitle: kırmızı |
| AU3 `AudioSources::default() == AudioSources { system: false }` | sesi söylemeyen çağıran Aşama 1 filmini alır | `Default`'u elle yazıp `system: true` koy: kırmızı |
| AU4 `include_str!("mod.rs")`: `fn record(` gövdesinde `recording_config(` çağrısı `audio` argümanını taşır ve gövdede `AudioSources::default()` geçmez | kayıt, çağıranın istediği sesi kullanır | çağrıya `AudioSources::default()` yaz ve `let _ = audio;` ekle: kırmızı |
| AU5 `recording.rs` ve `mod.rs` üretim kodunda (test modülü ve yorum satırları hariç) `SCStreamOutputType::Audio` ve `SCStreamOutputType::Microphone` hiç geçmez; dilimlemenin 20 satırdan uzun bir gövde bulduğu da iddia edilir (C4 deseni) | ses handler'ı, durdurma sırasının "handler kalmadı" koşulunu bozamaz | `record`'a `add_output_handler(.., SCStreamOutputType::Audio)` ekle: kırmızı |
| HD1 Yalnız test modülündeki yardımcı `fn sound_track_count(movie: &[u8]) -> usize`: `hdlr` etiketinden 12 bayt sonra `soun` geçen her konum bir izdir. Elle kurulmuş kutularla: tek `soun` → 1, tek `vide` → 0, `soun` + `soun` + `vide` → 2, `soun`'un 8. baytta olduğu hatalı kutu → 0 | C12 ve C13'ün sayacı gerçekten ses izini sayıyor (`hdlr` düzeni: boyut 4, tip 4, sürüm/bayrak 4, pre_defined 4, handler_type 4) | ofseti 12 yerine 8 yap: birinci ve dördüncü durum kırmızı |
| C12 `#[ignore]` **Gerçek donanım.** `SCRecordingOutput::is_available()` ve `/System/Library/Sounds/Glass.aiff`'in varlığı yüksek sesle iddia edilir. Ana display'de iki kayıt, her biri 4 saniye, kayıt boyunca bir iş parçacığı her 500 ms'de `/usr/bin/afplay /System/Library/Sounds/Glass.aiff` başlatır: (a) `AudioSources { system: true }`, (b) `AudioSources::default()`. **Her kayıt önce başladığını kanıtlar:** 2. saniyede `progress().frames > 0`; durunca `frames > 0`, `bytes > 0`, `has_moov_atom`. Dosyalar iddialardan önce silinir (C11 deseni). Sonra (a) `sound_track_count == 1`, (b) `sound_track_count == 0` | sistem sesi dosyada; sessiz yapılandırma, ses çalarken bile sessiz | `with_captures_audio(false)` sabitle: (a) kırmızı. `with_captures_audio(true)` sabitle: (b) kırmızı |

C12'nin (a) kolu 1'den farklı bir sayı verirse test "düzeltilmez": sayı ek kararlara yazılır ve raporlanır.

- [ ] **Step 2: Kırmızıyı gör, uygula.**
- [ ] **Step 3:** Tüm kapılar temiz. Rapora yaz:
  - `git diff --stat crates/capture/src/mock.rs` boş.
  - `git diff Cargo.lock` boş.
  - `git diff -U0 crates/capture/src/macos/recording.rs` içinde `fn tear_down`, `fn release`, `trait RecordingTeardown`, `impl SCStreamOutputTrait for FrameCounter`, `impl Drop for MacRecording` gövdelerine düşen hunk yok.
- [ ] **Step 4:** `cargo test -p snapdeck-capture --release -- --ignored` elle koşulur (terminale Screen & System Audio Recording izni gerekir). C11 ve C12'nin yeşil olduğu, C12'nin (a) ve (b) iz sayıları rapora yazılır.
- [ ] **Step 5: Commit** `feat(capture): take system audio into a recording`

---

### Task 2: `Record system audio` onay kutusu

**Birim:** U4. **Bağımlılık:** yok. **Görev 1 ile paralel.**

**Files:** `apps/desktop/src/settings/SettingsWindow.tsx`

**Interfaces (birebir):**

```ts
/** Mirrors `settings::Settings`. */
export type Settings = {
  // ...mevcut alanlar değişmez...
  /** Whether a recording takes in what the Mac is playing. */
  recordSystemAudio: boolean
}
```

**Kurallar:**

- `Behaviour` bölümünün hemen ardından yeni bir `<Section title="Recording">`; içinde tek `<Check label="Record system audio" checked={settings.recordSystemAudio} onChange={(recordSystemAudio) => update({ recordSystemAudio })} />`. İpucu metni yok.
- TypeScript'te varsayılan değer yazılmaz; değer `get_settings`'ten gelir (dosya başı yorumu ve `settings.rs`'in kuralı).
- Görev 3 birleşene kadar Rust alanı göndermez ve kutu tanımsız değerle çizilir. Bu yalnız iki commit arasında görülür; Görev 2 ve 3 aynı sürümde çıkar.

- [ ] **Step 1: Test yok, ve nedeni.** Bu görev saf mantık eklemiyor; mutasyon kanıtı olmayan test yazılmaz. Kablolamanın kanıtı Görev 3'ün ST4 testidir: bu dosyayı Rust'tan okur ve `checked={settings.recordSystemAudio}` ile `update({ recordSystemAudio })` dizelerini arar.
- [ ] **Step 2: Uygula.**
- [ ] **Step 3:** `pnpm lint`, `pnpm test`, `pnpm build` temiz.
- [ ] **Step 4: Commit** `feat(settings): add the system audio switch`

---

### Task 3: `record_system_audio` ayarı ve kaydın ayarı okuması

**Birim:** U3. **Bağımlılık:** Görev 1, Görev 2.

**Files:** `apps/desktop/src-tauri/src/{settings.rs,recording.rs,commands.rs}` (`commands.rs`'te yalnız test)

**Interfaces (birebir):**

```rust
// apps/desktop/src-tauri/src/settings.rs, Settings'e eklenen alan
    /// Whether a recording takes in what the Mac is playing.
    ///
    /// Off until turned on: recordings were silent before this existed, and
    /// sound is a choice about what ends up in a file somebody may share.
    /// macOS asks nothing for it; system audio sits under the screen
    /// recording grant.
    pub record_system_audio: bool,

// Default for Settings içinde
            record_system_audio: false,
```

```rust
// apps/desktop/src-tauri/src/recording.rs

/// The sound a recording takes in, from the settings in force when it starts.
fn audio_sources(settings: &Settings) -> AudioSources;
```

**Kurallar:**

- `start`'ın 3. adımında bir kez okunan `settings` kullanılır; ikinci bir `state.settings()` çağrısı yok. 7. adım `.record(CaptureTarget::Region(global), audio_sources(&settings), &files.temporary)` olur.
- `settings_to_store` değişmez: yeni alan `..requested` ile taşınır. ST5 bunu kanıtlar.
- `a_good_file_is_used_as_written`'daki struct literal'e `record_system_audio: true` eklenir (derleme için zorunlu; varsayılandan farklı değer gidiş-dönüşü kanıtlar). İddiaları değişmez.

- [ ] **Step 1: Testleri yaz**

| Test | Ne kanıtlar | Mutasyon kanıtı |
|---|---|---|
| ST1 `assert!(!Settings::default().record_system_audio)` | varsayılan sessiz | varsayılanı `true` yap: kırmızı |
| ST2 `serde_json::to_string(&Settings::default())` `"recordSystemAudio"` anahtarını içerir (literal) | pencerenin okuduğu ad | alana `#[serde(rename = "systemAudio")]` ekle: kırmızı |
| ST3 `settings_from_json(r#"{"filenameTemplate": "shot-{time}"}"#)` şikâyetsiz yüklenir ve `record_system_audio == false` | bu alandan önce yazılmış dosya çalışır | alana `true` döndüren bir `serde(default = ..)` ekle: kırmızı |
| ST4 `SETTINGS_UI` (settings.rs:678'deki mevcut sabit) `checked={settings.recordSystemAudio}` ve `update({ recordSystemAudio })` dizelerini içerir | onay kutusu gerçekten bu alanı okuyup yazıyor | TypeScript'te alan adını değiştir: kırmızı. `Check`'i sil: kırmızı |
| ST5 `settings_to_store(&previous, requested, false)` ve `settings_to_store(&previous, requested, true)`: `previous.record_system_audio == false`, `requested.record_system_audio == true` iken iki sonuç da `true` | işaretlenen kutu diske yazılır; kısayol reddi onu geri almaz | `settings_to_store`'a `record_system_audio: previous.record_system_audio` ekle: kırmızı |
| RA1 `audio_sources(&Settings { record_system_audio: true, ..Settings::default() }) == AudioSources { system: true }`; `audio_sources(&Settings::default()) == AudioSources { system: false }` | ayar kayda gidiyor | `AudioSources::default()` döndür: ilk iddia kırmızı |
| RA2 `body_of("pub fn start(")` içinde `.record(` çağrısı `audio_sources(&settings)` içerir | kablolama | `AudioSources::default()` geçir: kırmızı |

- [ ] **Step 2: Kırmızıyı gör, uygula.**
- [ ] **Step 3:** Tüm kapılar temiz; `git diff Cargo.lock` boş.
- [ ] **Step 4: Commit** `feat(app): record system audio when the setting is on`

---

### Task 4: Mikrofon ölçüm kapısı

**Birim:** D + U6. **Ürün kodu commit edilmez.** **Bağımlılık:** Görev 1 (gerçek boru hattı, sistem sesi kablolaması dahil). Görev 3 ile paralel koşar.

**Amaç.** Tasarım 3.2 ve 16 (S3): `captureMicrophone = true` iken `SCRecordingOutput`'un ürettiği MP4 oynatılabilir mi ve içinde ses izi var mı? Bu görev bunu Snapdeck'in **gerçek kayıt boru hattıyla** ölçer: `resolve`, `record`'un 1-7 adımları ve `tear_down` sırası. Karar bu görevin raporundadır; 2b'nin hiçbir uygulama görevi bu rapordan önce başlamaz.

#### 4.1 Ortam

- [ ] Sürüm **okunur, kopyalanmaz**: `sw_vers -productVersion`, `sw_vers -buildVersion`, `uname -r`, `sysctl -n hw.model`. Aşama 1 notu "macOS 27" diyor; bu planı yazan oturumun ortamı `Darwin 25.5.0` bildiriyordu. Hangisinin doğru olduğu burada ölçülür.
- [ ] macOS 15.x çalıştıran erişilebilir bir makine varsa aynı matris orada da koşulur. Yoksa raporda "macOS 15 koşulmadı" yazar (Açık sorular, S6).
- [ ] `xcrun --find swift` bir yol döndürmeli. Döndürmüyorsa ölçüm **engellenmiştir**: rapor edilir ve yerine "oynatılabilir" diyebilecek bir tahmin konmaz.

#### 4.2 Ölçüm aracı: `probe.swift` (scratch'te, commit edilmez)

AVFoundation ile, Aşama 1 Task 9'un `AVURLAsset` yöntemiyle aynı. Her çağrı tek satır `anahtar=değer` basar; dosyayı hiçbir oynatıcıda açmaz ve ses çalmaz.

| Çağrı | Okuduğu | Bastığı |
|---|---|---|
| `xcrun swift probe.swift --microphone-status` | `AVCaptureDevice.authorizationStatus(for: .audio).rawValue` | `mic_status=<n>` |
| `xcrun swift probe.swift --request-microphone` | `AVCaptureDevice.requestAccess(for: .audio)` (istem terminal uygulamasına atfedilir; kullanıcı onaylar) | `mic_granted=<true/false>` |
| `xcrun swift probe.swift <dosya>` | `AVURLAsset`: `load(.isPlayable, .duration)`; `loadTracks(withMediaType: .video)` sayısı, ilk izin `naturalSize`, `nominalFrameRate`, `timeRange`; `AVAssetReader` + `AVAssetReaderTrackOutput(outputSettings: nil)` ile video örneklerini sonuna kadar okur. `loadTracks(withMediaType: .audio)` sayısı ve her ses izi için `formatDescriptions` (`CMAudioFormatDescriptionGetStreamBasicDescription`: örnekleme hızı, kanal), `timeRange`, `isEnabled`; ayrıca `AVAssetReaderTrackOutput`'u Float32 LPCM çıkış ayarıyla sonuna kadar okur ve mutlak tepe değerini hesaplar | `playable`, `duration_s`, `video_tracks`, `video_size`, `video_fps`, `video_samples`, `video_reader`, `audio_tracks`, ve her ses izi için `audio<i>_rate`, `audio<i>_channels`, `audio<i>_duration_s`, `audio<i>_enabled`, `audio<i>_samples`, `audio<i>_reader`, `audio<i>_peak` |

`*_reader` değerleri `AVAssetReader.status`'tur (`completed` veya `failed`). `isPlayable`, bozuk bir dosyada da `true` diyebilir; bozulmayı asıl yakalayan, okuyucuların sonuna kadar gelmesidir.

#### 4.3 Worktree ve geçici yama

- [ ] `git worktree add <scratch>/snapdeck-mic-gate HEAD`. Ana çalışma ağacında hiçbir dosya değişmez; `CARGO_TARGET_DIR` paylaşılmaz.
- [ ] **Yalnız worktree'de, commit edilmeyen** iki değişiklik:
  1. `recording_config` zincirine `.with_captures_microphone(std::env::var_os("SNAPDECK_GATE_MICROPHONE").is_some())` (audio.rs:347, `macos_15_0` açık).
  2. `recording.rs` test modülüne `#[ignore]` bir `gate_recording` testi. `SNAPDECK_GATE_OUT` (dosya yolu), `SNAPDECK_GATE_SECONDS`, `SNAPDECK_GATE_SYSTEM` okur. Ana display'i `AudioSources { system }` ile kaydeder. Sürenin yarısında `frames_mid=<progress().frames>` ve `file_exists_mid=<bool>` basar. Durunca `frames`, `bytes` ve `wall_s` (`record()` dönüşünden `stop()` çağrısına kadar geçen süre) basar. **Dosyayı silmez**; silme, betik okuduktan hemen sonra kabuğun işidir.
- [ ] Komut: `SNAPDECK_GATE_OUT=<scratch>/gate/<koşu>.mp4 SNAPDECK_GATE_SECONDS=<s> SNAPDECK_GATE_SYSTEM=<0|1> [SNAPDECK_GATE_MICROPHONE=1] cargo test -p snapdeck-capture --release -- --ignored --exact macos::recording::tests::gate_recording --nocapture`
- [ ] Sistem sesi olan koşularda kabuk, kayıt boyunca her 500 ms'de `afplay /System/Library/Sounds/Glass.aiff` başlatır.

#### 4.4 Kullanıcı adımları (ajan yapamaz)

- [ ] Terminal uygulamasının Screen & System Audio Recording izni olduğunu kullanıcı doğrular (Aşama 1'in C11'i bunu istemişti).
- [ ] `--microphone-status` 3 değilse `--request-microphone` çalıştırılır ve kullanıcı istemde **Allow** der. Terminal uygulamasında `NSMicrophoneUsageDescription` yoksa süreç çöker; bu bir kapı sonucu değil ortam sorunudur ve Terminal.app'ten yeniden koşulur.
- [ ] Kullanıcıya önceden söylenir: toplam yaklaşık 2 dakika mikrofon kaydı alınacak ve menü çubuğunda mikrofon göstergesi yanacak. Kayıtlar dinlenmez, yalnız sayıları okunur ve hemen silinir. Oda sessiz kalabilir. Hoparlör ve giriş sesi kapalı olmamalı.

#### 4.5 Koşu matrisi (sırayla; her koşudan hemen sonra betik, sonra `rm -f`)

| Koşu | `SYSTEM` | `MICROPHONE` | Süre | Tekrar | `afplay` | Rolü |
|---|---|---|---|---|---|---|
| M0 | 0 | yok | 10 s | 1 | evet | kontrol: sessiz kalır |
| M1 | 1 | yok | 10 s | 3 | evet | kontrol: 2a çalışıyor |
| M2 | 0 | 1 | 10 s | 3 | hayır | **kapı** |
| M3 | 1 | 1 | 10 s | 3 | evet | **kapı** |
| M3-uzun | 1 | 1 | 60 s | 1 | evet | **kapı**: film sonlandırmada bozuluyorsa uzun kayıt, büyük `moov` ile görünür |

- [ ] **Betiğin öz denetimi (ölçümün mutasyon kanıtı):** M0 dosyası silinmeden önce `head -c <boyutun yarısı>` ile yarıya kesilmiş bir kopyası çıkarılır. Betik bu kopyada `playable=false` veya `video_reader=failed` demelidir. Demiyorsa betik geçersizdir ve hiçbir sonuç sayılmaz.

#### 4.6 Karar kuralı

**Geçerlilik önkoşulları** (her koşuda; tutmazsa sonuç **GEÇERSİZ**, sayılmaz):

- P1 `frames_mid > 0` ve `file_exists_mid=true`: kayıt gerçekten başladı.
- P2 `frames > 0`, `bytes > 0`.
- P3 M2, M3 ve M3-uzun öncesinde `mic_status=3`.
- P4 Betik öz denetimi geçti. M0 `playable=true` ve `audio_tracks=0` verdi. M1'in her koşusu `audio_tracks>=1` ve bir ses izinde `peak > 0.01` verdi. M1 bunu vermiyorsa bu bir 2a hatasıdır ve ayrıca raporlanır.

**GEÇTİ ölçütleri** (M2, M3 ve M3-uzun'un **her** koşusunda):

- G1 `playable=true`
- G2 `video_reader=completed` ve `video_samples > 0`
- G3 `audio_tracks >= 1`; her ses izinde `audio<i>_reader=completed` ve `audio<i>_samples > 0`
- G4 `|duration_s - wall_s| <= 0.5`
- G5 her ses izinde `audio<i>_duration_s >= duration_s - 0.5`
- G6 M2'de en az bir ses izinde `peak > 0` (dijital sessizlik değil; mikrofon örnekleri dosyaya gerçekten girdi)

**Sonuçlar:**

- **GEÇTİ:** önkoşullar ve G1-G6 her kapı koşusunda tuttu. Rapor yazılır; Görev 6, 7 ve 8 **CTO onay satırı yazılınca** başlar.
- **KALDI:** önkoşullar tutarken G1-G6'dan herhangi biri herhangi bir kapı koşusunda tutmadı. Tasarım 16, S3 uygulanır: mikrofon bu sürüme alınmaz, ayrı AAC + mux yoluna girilmez. Görev 6, 7 ve 8 iptal olur; Görev 9 yalnız belge olarak koşar.
- **GEÇERSİZ:** önkoşul tutmadı. Sebep giderilir ve matris yeniden koşulur. Üç kez üst üste GEÇERSİZ olursa ajan durur ve CTO'ya sorar.
- **Bayrak (karar değil):** M3'ün `audio_tracks` değeri. 2 çıkarsa sistem sesi ve mikrofon ayrı izlerdedir; raporda açıkça yazılır ve CTO onay satırında kabul edip etmediğini söyler (Açık sorular, S5).

#### 4.7 Temizlik ve rapor

- [ ] Her betik okumasından hemen sonra dosya silinir; matris bitince `ls <scratch>/gate` sıfır film gösterir (rapora yazılır).
- [ ] `git worktree remove --force <scratch>/snapdeck-mic-gate`. Ana ağaçta `git status --porcelain` bu görevden önceki haliyle aynıdır (rapora yazılır). Hiçbir şey commit edilmez.
- [ ] Bu dosyanın sonundaki **"Mikrofon ölçüm kapısı raporu"** şablonu doldurulur; ek kararlar bölümüne (U6) yazılan tek şey budur.

---

### Task 5: 2a gerçek donanım doğrulaması ve belgeler

**Birim:** U5 + D. **Bağımlılık:** Görev 3; D, Görev 4'ten boşalmış olmalı.

**Files:** `README.md`, `CHANGELOG.md`

- [ ] **Step 1:** Aşağıdaki "Gerçek donanım doğrulaması, 2a" tablosunun **her maddesi** release paketiyle koşulur ve sayılar rapora yazılır. Koşulamayan madde "koşulmadı, sebebi" diye yazılır.
- [ ] **Step 2: README:**
  - Satır 204 `Not in this release: sound (neither system audio nor the microphone), GIF output and trimming.` şöyle olur: `Not in this release: the microphone, GIF output and trimming.`
  - Recording bölümüne kısa bir paragraf eklenir: `Record system audio` ayarı; kaydın Mac'in çaldığı sesi aldığı; Snapdeck'in kendi sesinin alınmadığı; yeni izin istenmediği, çünkü sistem sesi Screen & System Audio Recording izninin altındadır.
  - Ölçüm tablosuna ses açıkken ölçülen dosya boyutu hızı eklenir (madde H6).
  - Satır 525-526'daki `**Recordings have no sound yet**` sınırlaması, mikrofonun henüz olmadığını söyleyecek şekilde güncellenir. macOS 15 cümlesi kalır.
- [ ] **Step 3: CHANGELOG:** `git tag --list v0.3.0` boşsa `## v0.3.0` bölümüne `### Added` altında sistem sesi maddesi yazılır ve `### Known limitations`'taki `No sound` satırı `No microphone` olur. Etiket varsa ajan **durur** ve sürüm numarasını CTO'ya sorar (Açık sorular, S3).
- [ ] **Step 4: Commit** `docs: document system audio in recordings`

---

### Task 6: Mikrofon izni ve `captureMicrophone` yapılandırması

**Birim:** U1 + U2 (+ U3'te yalnız derleme için zorunlu literal güncellemeleri). **Bağımlılık:** KAPI.

> **Önkoşul (kapı):** Başlamadan önce bu dosyanın "Mikrofon ölçüm kapısı raporu" bölümü okunur. `Sonuç: GEÇTİ` ve tarihli `CTO onayı` satırı yoksa görev başlamaz: ajan hiçbir dosyaya dokunmaz ve "kapı kapalı" diye döner.

**Files:** `crates/frame/src/types.rs`, `crates/capture/src/macos/{recording.rs,permission.rs}`, `crates/capture/Cargo.toml`, `apps/desktop/src-tauri/src/recording.rs` (yalnız `AudioSources { .. }` literal'lerine `microphone: false`)

**Kaynaktan doğrulanan:**

- `with_captures_microphone(bool)` audio.rs:347 (`#[cfg(feature = "macos_15_0")]` :345); `captures_microphone()` :354.
- Swift köprüsü: `StreamConfiguration.swift:645` `cfg.captureMicrophone = value`.
- Crate'te yetkilendirme API'si yok.
- `AVAuthorizationStatus` ham değerleri **SDK başlığından okunur**: `grep -rn "AVAuthorizationStatus" "$(xcrun --show-sdk-path)/System/Library/Frameworks/AVFoundation.framework/Headers/"`. Plan `NotDetermined = 0`, `Restricted = 1`, `Denied = 2`, `Authorized = 3` bekliyor; başlık başka bir şey derse başlık kazanır ve ek kararlara yazılır.

**Interfaces (birebir):**

```rust
// crates/frame/src/types.rs, AudioSources'a eklenen alan
    /// The default input device. It needs a grant of its own, which the caller
    /// asks for before the recording is built.
    pub microphone: bool,
```

```rust
// crates/capture/src/macos/recording.rs
// recording_config zincirine: .with_captures_microphone(audio.microphone)
```

```rust
// crates/capture/src/macos/permission.rs

/// The microphone grant as the system reports it, without prompting.
///
/// `NotDetermined` answers `Denied`: a microphone the system has not said yes
/// to is not one to record.
pub fn microphone_permission() -> PermissionState;

/// Asks for the microphone when macOS has never been asked, and answers with
/// the grant in force.
///
/// Blocks the calling thread until the user answers the system prompt, so it
/// is only ever called from a blocking worker. No timeout: the prompt is the
/// user's to answer, and a recording that starts before the answer would take
/// in a microphone nobody allowed yet. Prompts at most once for the life of
/// the grant; afterwards it answers without asking.
pub fn request_microphone_permission() -> PermissionState;

/// What `AVAuthorizationStatus` says, in the three states a caller acts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MicrophoneStatus {
    NotDetermined,
    Denied,
    Granted,
}

/// Pure. `Restricted` and any value this build does not know are `Denied`.
pub(crate) fn microphone_status_from_raw(raw: isize) -> MicrophoneStatus;

/// Whether the system prompt should be shown: only for a question never asked.
pub(crate) fn needs_request(status: MicrophoneStatus) -> bool;
```

```toml
# crates/capture/Cargo.toml, [target.'cfg(target_os = "macos")'.dependencies]
# The microphone grant is an AVFoundation question with a completion block,
# and ScreenCaptureKit's bridge does not ask it. Both crates are already in
# the lock file through the desktop application and `dispatch2`.
objc2 = { version = "0.6", default-features = false, features = ["std"] }
block2 = { version = "0.6", default-features = false, features = ["std"] }
```

**Kurallar:**

- `AVMediaTypeAudio` sembolü, dosyadaki CoreGraphics `extern` bloğuyla aynı desende `#[link(name = "AVFoundation", kind = "framework")]` altında bildirilir.
- `request_microphone_permission`: önce `microphone_status_from_raw`. Yalnız `needs_request` doğruysa `requestAccessForMediaType:completionHandler:` çağrılır. Blok (`block2::RcBlock`) cevabı bir `std::sync::mpsc` kanalına yollar ve `recv()` bekler; cevap `PermissionState`'e çevrilir.
- Hâlâ **hiçbir** `Microphone` output handler'ı yok; AU5 aynen tutar.
- Uygulamadaki `AudioSources { system: .. }` literal'leri (Görev 3'ün `audio_sources` fonksiyonu ve RA1) yalnız `microphone: false` alır; uygulama davranışı Görev 8'e kadar değişmez.

- [ ] **Step 1: Testleri yaz**

| Test | Ne kanıtlar | Mutasyon kanıtı |
|---|---|---|
| AU3 (güncellenir) `AudioSources::default() == AudioSources { system: false, microphone: false }` | varsayılan hâlâ sessiz | `Default`'ta `microphone: true`: kırmızı |
| AU6 `recording_config(800, 600, None, AudioSources { system: false, microphone: true })`: `captures_microphone() == true`, `captures_audio() == false`. `AudioSources::default()` ile `captures_microphone() == false` | mikrofon bayrağı bağlı ve sistem sesinden ayrı | `with_captures_microphone(false)` sabitle: kırmızı. `with_captures_microphone(audio.system)` yaz: kırmızı |
| MP1 `microphone_status_from_raw`: `0` → `NotDetermined`, `1` → `Denied`, `2` → `Denied`, `3` → `Granted`, `4` → `Denied`, `-1` → `Denied` (değerler SDK başlığından, literal) | izin eşlemesi | `1 => Granted`: kırmızı. `_ => Granted`: kırmızı |
| MP2 `needs_request`: yalnız `NotDetermined` için `true` | reddedilmiş izin yeniden sorulmaz, verilmiş olan hiç sorulmaz | `status != MicrophoneStatus::Granted` yaz: `Denied` durumu kırmızı |
| MP3 `include_str!("permission.rs")` üretim kodunda (yorumlar hariç) `requestAccessForMediaType` **tam bir kez** geçer ve `needs_request(` çağrısı ondan önce gelir | istem yalnız karar fonksiyonunun izniyle çıkar | `needs_request` kontrolünü atla: kırmızı |
| C13 `#[ignore]` **Gerçek donanım.** `SCRecordingOutput::is_available()` ve `microphone_permission().is_granted()` yüksek sesle iddia edilir ("grant the terminal the microphone first"). Ana display 5 s, `AudioSources { system: false, microphone: true }`. 2. saniyede `progress().frames > 0`; durunca `frames > 0`, `bytes > 0`, `has_moov_atom`. Dosya iddialardan önce silinir. `sound_track_count ==` **kapı raporundaki M2 `audio_tracks` değeri** (literal) | kapının ölçtüğü şeyin depodaki regresyon kanıtı | `with_captures_microphone(false)`: iz 0, kırmızı |

- [ ] **Step 2: Kırmızıyı gör, uygula.**
- [ ] **Step 3:** Tüm kapılar temiz. `git diff Cargo.lock` **yalnız** `snapdeck-capture` girişinin `dependencies` listesine `"block2"` ve `"objc2"` ekler; yeni `[[package]]` yok. Diff rapora yapıştırılır. Global Constraints'teki `tear_down` diff kontrolü tekrarlanır.
- [ ] **Step 4:** `cargo test -p snapdeck-capture --release -- --ignored` elle; C11, C12 ve C13 yeşil.
- [ ] **Step 5: Commit** `feat(capture): record the microphone into a recording`

---

### Task 7: `Record the microphone` onay kutusu

**Birim:** U4. **Bağımlılık:** KAPI. **Görev 6 ile paralel.**

> **Önkoşul (kapı):** Görev 6'daki önkoşulun aynısı.

**Files:** `apps/desktop/src/settings/SettingsWindow.tsx`

**Interfaces (birebir):**

```ts
  /**
   * Whether a recording takes in the default microphone. Turning it on asks
   * nothing; macOS asks when a recording that wants it starts.
   */
  recordMicrophone: boolean
```

**Kurallar:** `Recording` bölümünde, `Record system audio`'nun hemen altına `<Check label="Record the microphone" checked={settings.recordMicrophone} onChange={(recordMicrophone) => update({ recordMicrophone })} />`. İpucu metni yok. Test yok; gerekçe Görev 2'deki gibi, kablolama kanıtı Görev 8'in MS4 testi.

- [ ] **Step 1:** Uygula. **Step 2:** `pnpm lint`, `pnpm test`, `pnpm build` temiz. **Step 3: Commit** `feat(settings): add the microphone switch`

---

### Task 8: `record_microphone`, izin kararı, `Info.plist` ve imza yetkisi

**Birim:** U3. **Bağımlılık:** Görev 6, Görev 7.

> **Önkoşul (kapı):** Görev 6'daki önkoşulun aynısı.

**Files:** `apps/desktop/src-tauri/src/{settings.rs,recording.rs,commands.rs}` (`commands.rs`'te yalnız test), `apps/desktop/src-tauri/Info.plist`. **Koşullu:** `apps/desktop/src-tauri/Entitlements.plist` (yeni), `apps/desktop/src-tauri/tauri.conf.json`.

- [ ] **Step 0: İmza bayraklarını ölç (koşullu adımın girdisi).** CI'daki ad-hoc yolla aynı biçimde derle: `APPLE_SIGNING_IDENTITY=- pnpm tauri build`. Sonra `codesign --display --verbose=4 target/release/bundle/macos/Snapdeck.app 2>&1 | grep -i flags` ve `codesign -d --entitlements - --xml target/release/bundle/macos/Snapdeck.app`. Çıktılar ek kararlara yazılır.
  - `flags` içinde `runtime` **varsa**: `Entitlements.plist` yalnız `com.apple.security.device.audio-input = <true/>` anahtarıyla oluşturulur ve `tauri.conf.json`'da `bundle.macOS.entitlements: "Entitlements.plist"` ayarlanır. Alan adı `tauri-utils` 2.9.3 `config.rs:661`'den doğrulandı. IP2 testi yazılır.
  - `runtime` **yoksa**: dosya oluşturulmaz, IP2 yazılmaz, ölçüm ek kararlara yazılır.

**Interfaces (birebir):**

```rust
// apps/desktop/src-tauri/src/settings.rs, Settings
    /// Whether a recording takes in the default microphone.
    ///
    /// Off until turned on, and turning it on asks nothing: the system is
    /// asked when a recording that wants the microphone starts, which is the
    /// application's rule for every permission.
    pub record_microphone: bool,

// Default for Settings
            record_microphone: false,
```

```rust
// apps/desktop/src-tauri/src/recording.rs

/// What the user is told when a recording that asked for the microphone
/// starts without it.
const RECORDING_WITHOUT_MICROPHONE: &str = "Recording without the microphone: Snapdeck is not allowed to use it. Turn Snapdeck on in System Settings > Privacy & Security > Microphone, then start a new recording.";

/// Asks for the microphone only when this recording wants it, and answers
/// `None` when it does not, in which case nothing was asked.
///
/// The request is injected for the reason `settings::apply_with` injects its
/// side effects: the system prompt is not something a test can answer.
fn microphone_grant<R>(settings: &Settings, request: R) -> Option<PermissionState>
where
    R: FnOnce() -> PermissionState;

/// The sound a recording takes in, and what to tell the user about a part of
/// it they asked for and will not get.
fn audio_sources(
    settings: &Settings,
    microphone: Option<PermissionState>,
) -> (AudioSources, Option<&'static str>);
```

```xml
<!-- apps/desktop/src-tauri/Info.plist, LSUIElement'in yanına -->
<key>NSMicrophoneUsageDescription</key>
<string>Snapdeck uses the microphone only while you record the screen with Record the microphone turned on in Settings.</string>
```

**Kurallar:**

- `audio_sources`'un kararı, sırayla:
  1. `record_microphone == false` → `microphone: false`, şikâyet `None`; `microphone` argümanı ne derse desin.
  2. `record_microphone == true` ve `Some(Granted)` → `microphone: true`, şikâyet `None`.
  3. `record_microphone == true` ve `Some(Denied)` ya da `None` → `microphone: false`, şikâyet `Some(RECORDING_WITHOUT_MICROPHONE)`.
  - `system` her durumda `record_system_audio`'dur.
- `start`'ta sıra: 3. adımda okunan `settings` → yeni 3a adımı `let microphone = microphone_grant(&settings, request_microphone_permission);` → `let (audio, microphone_complaint) = audio_sources(&settings, microphone);`. İkisi de süpürme ve `reserve`'den (4-6. adımlar) **önce** gelir: istem cevaplanana kadar kullanıcının klasöründe yer tutucu durmaz. 7. adım `.record(.., audio, ..)`. Şikâyet yalnız `begin_recording` başarılı olduktan sonra, 9. adımda `report_failure(app, microphone_complaint)` ile verilir: başlamayan bir kayıt için "mikrofonsuz kaydediyor" demek yanlış olur.
- Şikâyet Aşama 1'in "hata başlığı kazanır" kuralıyla gösterilir; görünür kaldıkça sayaç gizlenir (Açık sorular, S2). Bu görev o kuralı değiştirmez.
- `Info.plist`'teki `LSUIElement` kalır.
- `a_good_file_is_used_as_written` literal'ine `record_microphone: true` eklenir.

- [ ] **Step 1: Testleri yaz**

| Test | Ne kanıtlar | Mutasyon kanıtı |
|---|---|---|
| MS1 `assert!(!Settings::default().record_microphone)` | v1 kuralı: açılmamış kutu için istem yok | varsayılanı `true` yap: kırmızı |
| MS2 JSON `"recordMicrophone"` anahtarını içerir | pencerenin okuduğu ad | `#[serde(rename = "microphone")]`: kırmızı |
| MS3 `{"filenameTemplate": "shot-{time}"}` şikâyetsiz yüklenir, `record_microphone == false` | eski dosya | alana `true` döndüren bir `serde(default = ..)` ekle: kırmızı |
| MS4 `SETTINGS_UI` `checked={settings.recordMicrophone}` ve `update({ recordMicrophone })` içerir | kutu bu alana bağlı | TypeScript alan adını değiştir: kırmızı |
| MS5 `settings_to_store`, `refused` false ve true iken `requested.record_microphone`'u (true) korur | kutu diske yazılır | `previous`'tan al: kırmızı |
| MA1 `microphone_grant(&Settings::default(), \|\| panic!("nothing may be asked"))` `None` döner | kutuyu açmamış kullanıcıya istem çıkmaz | isteği koşulsuz çağır: panik, kırmızı |
| MA2 `record_microphone: true` iken `microphone_grant` kapanışın cevabını döndürür: `\|\| PermissionState::Granted` → `Some(Granted)`, `\|\| PermissionState::Denied` → `Some(Denied)` | cevap yutulmuyor | `Some(PermissionState::Granted)` sabitle: ikinci durum kırmızı |
| MA3 `audio_sources` doğruluk tablosu, beklentiler literal: (kutu kapalı, `Some(Granted)`) → (`microphone: false`, `None`); (kutu açık, `Some(Granted)`) → (`true`, `None`); (kutu açık, `Some(Denied)`) → (`false`, şikâyet `"without the microphone"` ve `"Privacy & Security > Microphone"` içerir); (kutu açık, `None`) → (`false`, şikâyet var); her satırda `system` ayardan | tasarım 8.3 ve 11 | `Denied` kolunda `microphone: true`: kırmızı. Şikâyeti `None` yap: kırmızı. Kutu kapalıyken `Granted`'ı `true` yap: kırmızı |
| MA4 `body_of("pub fn start(")`: `microphone_grant(` çağrısı `reserve(` ve `.record(`'dan önce geçer | istem, yer tutucudan ve akıştan önce | çağrıyı `.record(`'dan sonraya taşı: kırmızı |
| MA5 `body_of("pub fn start(")`: `microphone_complaint` taşıyan `report_failure(` çağrısı `begin_recording(`'dan sonra geçer | başlamayan kayıt için şikâyet yok | çağrıyı `.record(`'dan önceye taşı: kırmızı |
| IP1 `include_str!("../Info.plist")`: `<key>NSMicrophoneUsageDescription</key>`'ten hemen sonra boş olmayan bir `<string>` gelir ve `<key>LSUIElement</key>` hâlâ vardır | anahtar eksikse macOS süreci ilk mikrofon erişiminde öldürür (tasarım 8.3) | anahtarı sil: kırmızı. `<string></string>` yap: kırmızı |
| IP2 (yalnız Step 0'da `runtime` görüldüyse) `tauri.conf.json` `serde_json` ile okunur, `bundle.macOS.entitlements == "Entitlements.plist"`; dosya `com.apple.security.device.audio-input` anahtarını `<true/>` ile taşır | hardened runtime altında mikrofon | anahtarı sil: kırmızı. Yapılandırma satırını sil: kırmızı |

- [ ] **Step 2: Kırmızıyı gör, uygula.**
- [ ] **Step 3:** Tüm kapılar temiz.
- [ ] **Step 4: Commit** `feat(app): record the microphone and say so when it is not allowed`

---

### Task 9: 2b gerçek donanım doğrulaması ve belgeler

**Birim:** U5 + D. **Bağımlılık:** Görev 8 ve Görev 5. **KALDI yolunda:** yalnız Step 2b ve Step 3 koşar, donanım yok.

**Files:** `README.md`, `CHANGELOG.md`

- [ ] **Step 1 (GEÇTİ):** "Gerçek donanım doğrulaması, 2b" tablosunun her maddesi release paketiyle koşulur ve sayılar rapora yazılır.
- [ ] **Step 2a (GEÇTİ): README:**
  - Recording bölümüne mikrofon paragrafı: ayar; ilk kayıtta macOS'un izin sorduğu; izin yoksa kaydın mikrofonsuz başladığı ve bunu söylediği.
  - Satır 60-65'teki ad-hoc paragrafı, **mikrofon izninin de** her güncellemeden sonra yeniden istendiğini ve System Settings > Privacy & Security > Microphone yolunu söyler (tasarım 8.4, madde B8'in gözlemiyle).
  - M3 iki iz verdiyse bu bir cümleyle söylenir.
  - Satır 204 ve 525'teki "microphone" sınırlaması kalkar.
- [ ] **Step 2b (KALDI):** README ve CHANGELOG, mikrofonun bu sürümde olmadığını söyler (Görev 5'in cümleleri zaten bunu diyor; yalnız doğrulanır). Kapı raporunun sonucu CHANGELOG'a bir cümleyle yazılmaz: bu bir iç karardır.
- [ ] **Step 3: CHANGELOG:** Görev 5'in sürüm kuralıyla.
- [ ] **Step 4: Commit** `docs: document microphone recording` (KALDI yolunda değişiklik yoksa commit yok; bu rapora yazılır)

---

## Gerçek donanım doğrulaması

Bu bölümün hiçbir maddesi CI'da koşmaz ve hiçbiri bir birim testinin yerini tutmaz.

**Ortak kurallar:**

- Hepsi **release** paketiyle koşulur (Görev 4 hariç; o, gerçek boru hattını `cargo test --release` ile ölçer).
- Her ölçümden önce kaydın başladığı kanıtlanır: gizli `.snapdeck-recording-` geçici dosyası ve sıfır baytlık yer tutucu klasörde görülür. Görülmezse ölçüm geçersizdir (Aşama 1 Task 9'un ilk koşu dersi).
- Kayıtlar açılıp izlenmez ve dinlenmez. Sayılar `probe.swift` (Görev 4.2 sözleşmesi) ile okunur ve dosya hemen silinir.
- Ad-hoc imzalı yeni bir derleme kurulduktan sonra kullanıcı Screen & System Audio Recording iznini (ve 2b'de mikrofon iznini) yeniden verir ve Snapdeck'i kapatıp açar.

### 2a (Görev 5)

| # | Ölçüm | Nasıl | Raporlanacak sayı / eşik |
|---|---|---|---|
| H1 | Kaydın başladığı | her maddeden önce | geçici dosya ve yer tutucu ikisi de görüldü |
| H2 | Sistem sesi dosyada | ayar açık, tam ekran, 10 s, `afplay` döngüsü; 3 tekrar | `audio_tracks` (C12 ile aynı olmalı), `peak > 0.01`, `playable=true`, iki okuyucu `completed`, `\|duration_s - duvar saati\| <= 0.5` |
| H3 | Ayar kapalıyken sessiz | aynı, ayar kapalı; 1 tekrar | `audio_tracks=0` |
| H4 | Snapdeck'in kendi sesi | uygulama ses çalmıyor | "gözlenemez, AU1 ile kanıtlı" yazılır |
| H5 | Durdurma gecikmesi, ses açık | Aşama 1 madde 1 yöntemi, 5 tekrar | medyan ve p95, ms. **> 2000 ms ise bayrak** |
| H6 | Dosya boyutu hızı | 60 s hareketli masaüstü ve `afplay`, ses açık ve kapalı | iki sayı, MB/s; README'ye |
| H7 | İptal, ses açık | 5 s sonra `Cancel Recording` | 0 film, 0 geçici dosya, 0 yer tutucu |
| H8 | Mikrofon istemi çıkmıyor | ayar açık ilk kayıt | kullanıcı gözlemi: istem yok ve System Settings > Microphone listesinde Snapdeck yok (tasarım 8.2) |
| H9 | Ayar kalıcı | kutuyu aç, `Save`, uygulamayı kapat ve aç | kutu açık |
| H10 | DMG boyutu | bu aşamadan önce ve sonra | iki sayı, MB; fark gürültü içinde |

### Mikrofon ölçüm kapısı (Görev 4)

Görev 4'ün 4.1-4.7 bölümleri, bu bölümün kapı kısmıdır ve orada birebir tanımlıdır.

### 2b (Görev 9, yalnız GEÇTİ)

| # | Ölçüm | Nasıl | Raporlanacak sayı / eşik |
|---|---|---|---|
| B0 | İmza | `codesign --display --verbose=4`, `codesign -d --entitlements - --xml` | `runtime` bayrağı var mı; varsa `com.apple.security.device.audio-input` görünüyor mu |
| B1 | Info.plist birleşti | `plutil -extract NSMicrophoneUsageDescription raw <app>/Contents/Info.plist`, `plutil -extract LSUIElement raw ...` | metin basılır; `true` |
| B2 | İlk kullanım istemi | kullanıcı onayıyla `tccutil reset Microphone com.snapdeck.app`; kutu açık; `Record Full Screen`, `Enter` | istem `Enter`'dan sonra ve sayaç başlamadan çıkar (gözlem); kullanıcı **Allow** |
| B3 | Mikrofon dosyada | kutu açık, sistem sesi kapalı, 10 s; 3 tekrar | `audio_tracks` = kapı M2 değeri, `peak > 0`, `playable=true`, okuyucular `completed`, süre farkı <= 0,5 s |
| B4 | Sistem sesi ve mikrofon | ikisi açık, `afplay`, 10 s; 3 tekrar | `audio_tracks` = kapı M3 değeri, aynı ölçütler |
| B5 | Reddetme | reset, `Enter`, kullanıcı **Don't Allow** | kayıt başlar (H1), tepsi ipucu `without the microphone` ve `Privacy & Security > Microphone` içerir, `Stop` sonrası `playable=true` ve sistem sesi kapalıysa `audio_tracks=0` |
| B6 | Önceden reddedilmiş | aynı durumda ikinci kayıt | istem yok, aynı şikâyet hemen |
| B7 | Kutu kapalı | izin reset'li, kutu kapalı, kayıt | istem **çıkmaz** |
| B8 | Ad-hoc yeniden derleme | izin verilmişken yeniden derle, kur, kutu açık kayıt | istem yeniden çıkar (tasarım 8.4); README'ye yazılır |
| B9 | Durdurma gecikmesi, mikrofon açık | 5 tekrar | medyan ve p95, ms; **> 2000 ms ise bayrak** |
| B10 | İptal, mikrofon açık | 5 s sonra `Cancel Recording` | 0 dosya |
| B11 | Mikrofonu olmayan Mac | erişilebilirse | yoksa "koşulmadı" |
| B12 | DMG boyutu | Görev 5'in sayısına karşı | fark, MB |

---

## Kullanıcının yapması gerekenler

Ajanlar bunları yapamaz (TCC onayı, Touch ID veya parola, fiziksel onay):

1. **Görev 1 ve 6'nın `--ignored` testleri, Görev 4:** terminal uygulamasına Screen & System Audio Recording izni (verilmemişse ver, terminali yeniden aç). Görev 4 ve 6 için terminal uygulamasına mikrofon izni: `probe.swift --request-microphone` istemine **Allow**.
2. **Görev 4 öncesi:** yaklaşık 2 dakika mikrofon kaydı alınacağını kabul et. Hoparlör ve giriş sesi kapalı olmasın. macOS 15.x makine varsa bildir.
3. **Görev 5 ve 9 (her ad-hoc release kurulumu sonrası):** Screen & System Audio Recording iznini yeniden ver ve Snapdeck'i kapatıp aç.
4. **Görev 9:** `tccutil reset Microphone com.snapdeck.app` komutunu onayla. B2'de **Allow**, B5'te **Don't Allow** de. B8'deki yeniden derleme sonrası istemi yeniden onayla.
5. **KAPI:** Görev 4 raporunu oku; S5 ve S6'yı cevaplayarak onay ver ya da verme. Onay satırını CTO yazar.

---

## Uygulama sırasında alınan ek kararlar

### CTO kararları, Açık sorular S1-S8 (2026-09-14)

- **S1, kabul.** İki ayarın varsayılanı `false`. Aşama 1'in sessiz filmi korunur ve mikrofon için v1
  kuralı zaten bunu ister.
- **S2, kabul, ve tasarım 9.2 bu noktada geçersiz.** Aşama 1'de uygulanan ve testle çivilenen kural
  "hata tepsi başlığını kazanır"dır; tasarımın "kayıt sürerken başlık sayacındır" cümlesi bu
  kuraldan önce yazıldı. Mikrofon reddi `report_failure` ile verilir ve görünürken sayaç gizlenir.
- **S3, plandan farklı karar.** `v0.3.0` 2026-09-14'te etiketlendi ve **yayınlandı**. Ses bir sonraki
  sürümün, `## v0.4.0` bölümünün girdisidir. Ajan durup sormaz, `v0.4.0` bölümünü yazar; bölüm yoksa
  CHANGELOG'daki mevcut desenle oluşturur. Sürüm numarası yükseltmesi ve etiket bu planın işi değil.
- **S4, kabul.** `probe.swift` depoya girmez, scratch'te yaşar. Mikrofon kaydı alan bir ölçüm aracının
  depoda durması, birinin onu yanlışlıkla çalıştırması demektir.
- **S5, kabul, bayrak olarak.** M3'te iki ayrı ses izi raporlanırsa bu kapıda açıkça yazılır ve onay
  satırında karara bağlanır; önceden kabul edilmez.
- **S6, kabul.** Bu makine **macOS 26.5.2 (25F84)**; macOS 15.x makine yok. Kapı yalnız bu sürümde
  ölçülür ve onay satırı bunu açıkça kabul etmeden 2b başlamaz. README mikrofonun yalnız bu sürümde
  doğrulandığını söyler.
- **S7, kabul.** `Recording` bölümü macOS 14'te gizlenmez.
- **S8, kabul.** `Privacy_Microphone` derin bağlantısı bir yüzeye bağlanmaz; şikâyet metni ayar yolunu
  adıyla söyler.

Ek not: planın Aşama 1'e dair iki gözlemi doğrulandı ve düzeltildi. `MacRecording`'in doc yorumu
`failure_to_report` eklenirken yerinden kaymıştı, yerine döndü. Aşama 1 ölçüm notları test makinesini
"macOS 27" diye kaydetmişti; `sw_vers` 26.5.2 diyor, düzeltildi. `tauri.conf.json`'un 0.2.0 dediği
gözlemi ise eskiydi: ajan dosyayı sürüm yükseltmesinden önce okumuş, dosya 0.3.0.


> Uygulayıcılar bu bölümü doldurur. Plandan farklı bir karar verildiyse karar ve gerekçesi tek paragraf olarak buraya yazılır. Planla çelişen bir şey bulunduysa **uygulanmaz**, buraya yazılır ve rapor edilir.

### Mikrofon ölçüm kapısı raporu

- Tarih:
- Makine (`sysctl -n hw.model`):
- macOS (`sw_vers -productVersion` / `-buildVersion`), `uname -r`:
- macOS 15.x koşuldu mu:
- Betik öz denetimi (yarıya kesilmiş M0 kopyası):
- `mic_status` (kapı koşularından önce):

| Koşu | frames_mid | frames | bytes | wall_s | playable | duration_s | video_reader / samples | audio_tracks | her ses izi: reader / samples / duration_s / rate / channels / peak |
|---|---|---|---|---|---|---|---|---|---|
| M0 | | | | | | | | | |
| M1-1..3 | | | | | | | | | |
| M2-1..3 | | | | | | | | | |
| M3-1..3 | | | | | | | | | |
| M3-uzun | | | | | | | | | |

- Önkoşullar P1-P4:
- Ölçütler G1-G6:
- Bayrak, M3 `audio_tracks`:
- Silme doğrulaması (`ls` sayısı), `git status --porcelain` karşılaştırması:
- **Sonuç:** GEÇTİ / KALDI / GEÇERSİZ
- **CTO onayı:** (tarih; S5 ve S6 cevapları; yalnız CTO yazar)
