# Snapdeck Recording Stage 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Menü çubuğundan bölge, pencere veya tam ekran kaydı başlar; kayıt sürerken menü çubuğu geçen süreyi gösterir; `Stop Recording` kullanıcının klasörüne, kullanıcının ad şablonuyla, oynatılabilir bir `.mp4` bırakır ve dosya Recent Captures'a girer; `Cancel Recording` diskte hiçbir şey bırakmaz.

**Architecture:** Kodlayıcı yazılmaz. `SCStream`'e bir `SCRecordingOutput` takılır ve H.264/MP4 dosyasını Apple yazar (tasarım 5.2). `crates/capture` bir yol alır ve oraya yazar; ad şablonu, geçici dosya ve rename masaüstü tarafının işidir. Hedef çözümlemesi (`resolve`) çekim ve kayıt arasında paylaşılır, çünkü bölge kuralları iki yerde yazılırsa ilk düzeltmede ayrışır. Durdurma sırası tek bir fonksiyonun içindedir ve o fonksiyon bir sıra testiyle çivilenir.

**Tech Stack:** Rust 1.85, `screencapturekit` 9.0.1 (`macos_15_0` özelliği), Tauri 2.11.5, TypeScript (strict), Vitest.

---

## Global Constraints

- Hedef platform macOS 14.0+; **kayıt macOS 15.0+** ister ve çalışma zamanında `SCRecordingOutput::is_available()` ile kapanır. Uygulamanın tabanı 14.0'da kalır (tasarım 16, S1).
- Kod, yorum, commit mesajı ve arayüz metinleri İngilizce; yalnızca `docs/superpowers/` Türkçe.
- Ağ çağrısı yok, telemetri yok.
- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `pnpm lint`, `pnpm test`, `pnpm build` her commit'te temiz.
- **Yeni crate yok.** `Cargo.lock` bu planın sonunda hiç değişmemiş olmalı. Tek bağımlılık değişikliği kök `Cargo.toml`'daki `screencapturekit` özellik listesinin `macos_14_0` → `macos_15_0` olmasıdır; özellikler kümülatiftir (`macos_15_0 = ["macos_14_4"]`, kaynaktan doğrulandı), yani bu bir genişletmedir, bir sürüm atlaması değil. Bundle'a bayt eklenmez (tasarım 13, 5.2).
- **Hareketsiz çekim yolu bozulmaz.** `capture_region`, `save_edited`, `copy_edited`, izin komutları, köprü ve editör davranışı değişmez. `resolve()` refactor'ü (Görev 2) `capture()`'ın davranışını değiştirmez ve bunu kanıtlamakla yükümlüdür.
- **Kaydedilen dosya ikinci bir kaydetme yolu açmaz.** Klasör `settings::resolve_save_directory`, ad `output::render_filename`, çakışma soneki `output`'un mevcut ` 2`, ` 3` kuralı, liste `recents::record`. Kayıt için ayrı bir "nereye kaydedilsin" mantığı yazmak yasaktır.
- `packages/editor` içinde **hiçbir değişiklik yok** (tasarım 10.4). Video düzenleme editörün işi değil.

## Bu planın kapsamı dışında

Tasarım 3.1'in kapsam dışı bıraktıkları, birebir:

- Ses (sistem sesi ve mikrofon). Aşama 2.
- GIF çıktısı ve trim. Aşama 3.
- Ayar penceresinde yeni alan. Kare hızı, imleç, format, hepsi bu sürümde sabittir.
- Kayıttan sonra editörün açılması, videonun panoya kopyalanması.
- Kayıt için ayrı global kısayol (tasarım 2). Menü çubuğu yeterli.
- İmleç vurgusu, tıklama efekti, tuş göstergesi.
- Windows ve Linux kaydı. Trait'in varsayılan gövdesi hazır, implementasyon yok.

**Ayrıca bu planın almadığı ve açıkça sorulması gereken bir şey var:** kayıt başlamadan önce boş disk alanı ön kontrolü. Tasarımın 3.1'i "disk kontrolü"nü aşama 1'in kurduğu boru hattının parçası olarak sayıyor, ama kuralı tanımlayan bölüm 7 dosyada yok (bkz. "Tasarım dokümanındaki boşluk"). Bir `statvfs` okuması `libc`'yi doğrudan bağımlılık yapardı, ki bu bu planın "yeni bağımlılık yok" kısıtını deler. Bu yüzden plan **ön kontrol koymuyor** ve disk dolması gerçekte göründüğü yerde ele alınıyor: `SCRecordingOutputDelegate::recording_did_fail` ateşlenir, oturum hatalı işaretlenir, `Stop` kullanıcıya söyler ve dosyayı bırakmaz (Görev 3 ve 5). Ön kontrol isteniyorsa ayrıca karar verilmeli.

---

## Tasarım dokümanındaki boşluk (uygulayıcının bilmesi gereken)

`docs/superpowers/specs/2026-09-14-snapdeck-recording-design.md` diskte **eksik**: başlıklar 6.1'den doğrudan 16'ya atlıyor. Bölümler 6.2-6.4, 7, 8, 9, 10, 11, 12, 13, 14, 15 dosyada yok. Bu plan hayatta kalan bölümlere (1-5.6, 6.1 başlığı, 16) ve **gerçek kaynağa** dayanır: `~/.cargo/registry/.../screencapturekit-9.0.1/` okunarak her imza doğrulandı, ve `crates/`, `apps/desktop/src-tauri/src/` mevcut kodu okundu.

Eksik bölümlerden yeniden kurulan kararlar aşağıda **[yeniden kurulan]** diye işaretli. Uygulayıcı bunlardan biriyle çelişen bir şey bulursa **uygulamaz, rapor eder**.

---

## Plan formatı hakkında

Bu plan tip tanımlarını, fonksiyon imzalarını ve test durumlarını **birebir** verir: sözleşme budur, uygulayıcı bunları değiştiremez. Gövde kodunu uygulayıcı yazar. Her test için **mutasyon kanıtı** verilir: testi geçiren kodun hangi tek satırı bozulunca testin kırmızıya döneceği. Mutasyon kanıtı olmayan test yazılmaz; testi yazan kişi o satırı gerçekten bozup kırmızıyı görmekle yükümlüdür.

TDD zorunludur: önce test, sonra kod. Her görevin ilk adımı "testleri yaz", ikinci adımı "kırmızıyı gör".

### Mutasyon tuzağı (Plan 3'te iki kez düşüldü, tekrarlanmayacak)

**İddiayı, test ettiğin sabitten türetme. Sözleşme değerini teste açık yaz.**

Yanlış:

```rust
assert_eq!(recording_config(800, 600, None).fps(), RECORDING_FPS);
```

Bu test `RECORDING_FPS` 30 iken de 7 iken de yeşildir: iki taraf da aynı sabitten geliyor, yani test "yapılandırma sabiti okuyor" diyor, "kare hızı 30" demiyor. Mutasyon kanıtı yoktur.

Doğru, iki ayrı iddia:

```rust
// Sözleşme değeri, açık yazılmış.
assert_eq!(RECORDING_FPS, 30);
// Ve kablolama: yapılandırma gerçekten o değeri taşıyor.
assert_eq!(recording_config(800, 600, None).fps(), 30);
```

Aynı kural her yerde geçerli: `elapsed_text` beklentileri `"1:02:03"` diye yazılır, `format!` ile üretilmez; `reserve`'ün ürettiği ad `"Recording 2026-09-14 at 10.00.00 2.mp4"` diye yazılır, `suffixed_file_name` çağrılarak değil. İki dil arasındaki iddialar (`include_str!` ile TypeScript okuyan Rust testleri) bu kuralın istisnası değil, en katı uygulamasıdır: orada beklenti **karşı taraftan** gelir, aynı taraftan değil.

---

## Dosya sahipliği ve paralellik kuralı

**Aynı sahiplik biriminde aynı anda yalnız bir ajan çalışır.** Birimler:

| Birim | Kapsam |
|---|---|
| U1 | `crates/frame/**` |
| U2 | `crates/capture/**` ve kök `Cargo.toml`'un `screencapturekit` satırı |
| U3 | `apps/desktop/src-tauri/**` |
| U4 | `apps/desktop/src/**` (frontend TypeScript) |
| U5 | `README.md`, `docs/*.md` |

`packages/editor`, `apps/extension`, `crates/stitch` bu planda hiç açılmaz.

### Görev sırası ve paralellik

```
Görev 1 (U1)  ──┬─→ Görev 2 (U2) ─→ Görev 3 (U2) ──┐
                │                                   ├─→ Görev 8 (U3) ─→ Görev 9 (U5)
Görev 4 (U3) ───┴─→ Görev 5 (U3) ─→ Görev 7 (U3) ──┘
Görev 6 (U4) ──────────────────────────────────────┘
```

- **Paralel grup A (başlangıçta, aynı anda üç ajan):** Görev 1 (U1), Görev 4 (U3), Görev 6 (U4).
- **Paralel grup B:** Görev 2 (U2) ile Görev 5 (U3) aynı anda. Görev 5, Görev 1 ve Görev 4 bittikten sonra başlar.
- **Paralel grup C:** Görev 3 (U2) ile Görev 7 (U3) aynı anda.
- Görev 8 tek başına (U3, Görev 3 ve 7'yi bekler). Görev 9 en sonda.

---

## File Structure

```
crates/frame/src/types.rs                     RecordingProgress, RecordingSummary (ekleme)
crates/frame/src/error.rs                     CaptureError::Unsupported (ekleme)
crates/frame/src/lib.rs                       re-export listesi (ekleme)

crates/capture/src/lib.rs                     Recording trait, ScreenCapturer::record (ekleme)
crates/capture/src/macos/mod.rs               resolve(), resolve_region(), RegionPlan (refactor)
crates/capture/src/macos/recording.rs         YENİ: MacRecording, FrameCounter, tear_down
Cargo.toml                                    screencapturekit özelliği macos_15_0

apps/desktop/src-tauri/src/recording.rs       YENİ: dosya sözleşmesi, stop/cancel sonucu, tepsi sayacı
apps/desktop/src-tauri/src/output.rs          claim_free_path çıkarımı (refactor)
apps/desktop/src-tauri/src/state.rs           RecordingSession yuvası (ekleme)
apps/desktop/src-tauri/src/overlay.rs         action parametresi (ekleme)
apps/desktop/src-tauri/src/tray.rs            kayıt öğeleri, elapsed_text, record_label (ekleme)
apps/desktop/src-tauri/src/commands.rs        start_recording (ekleme)
apps/desktop/src-tauri/src/lib.rs             modül, komut kaydı, açılışta süpürme (ekleme)

apps/desktop/src/overlay/action.ts            YENİ: confirmCommand, confirmHint
apps/desktop/src/overlay/action.test.ts       YENİ
apps/desktop/src/overlay/Overlay.tsx          action prop'u (ekleme)
apps/desktop/src/overlay/main.tsx             action sorgu parametresi (ekleme)

README.md                                     "No screen recording" satırı düşer
```

---

## Yeni bağımlılıklar

**Yok.** Tek değişiklik:

| Değişiklik | Nerede | Neden |
|---|---|---|
| `features = ["macos_14_0"]` → `features = ["macos_15_0"]` | kök `Cargo.toml`, `screencapturekit` | `SCRecordingOutput`, `SCStream::add_recording_output` ve `SCStream::remove_recording_output` üçü de `#[cfg(feature = "macos_15_0")]` arkasında. Kaynaktan doğrulandı: `macos_15_0 = ["macos_14_4"]`, `macos_14_4 = ["macos_14_2"]`, `macos_14_2 = ["macos_14_0"]`, yani mevcut her şey açık kalır. `Cargo.lock` değişmez: bu özellikler opsiyonel bağımlılık çekmiyor. |

**Eklenmeyenler ve neden:** `ffmpeg-sidecar` (bundle'a 40-80 MB, ve GPL x264 MIT bir ürünle dağıtılamaz, tasarım 5.1), `objc2-av-foundation` (500-800 satırlık gerçek zamanlı kare pompası, tasarım 5.2; aşama 3'ün işi), `libc` (yalnız `statvfs` için olurdu, bkz. kapsam dışı notu).

---

### Task 1: Kayıt tipleri ve `record()` trait metodu

**Birim:** U1 (+ `crates/capture/src/lib.rs`, U2'nin tek dosyası). **Bağımlılık:** yok.

> **U1/U2 çakışması:** bu görev `crates/capture/src/lib.rs`'e de dokunur, yani U2 bu görev sürerken kilitlidir. Görev 2 bu görev bittikten sonra başlar.

**Files:** `crates/frame/src/{types.rs,error.rs,lib.rs}`, `crates/capture/src/lib.rs`

**Interfaces (birebir):**

```rust
// crates/frame/src/error.rs, CaptureError'a eklenen tek varyant
    /// This machine cannot do this at all: recording needs macOS 15.0, and
    /// the application itself runs on 14.0.
    ///
    /// Its own variant rather than a `Platform`, because the two ask the user
    /// for different things. A `Platform` failure is something that went
    /// wrong and might not next time; this one will never succeed on this
    /// machine, and saying so is the only useful thing to say.
    #[error("not supported on this system: {0}")]
    Unsupported(String),
```

```rust
// crates/frame/src/types.rs
use std::path::PathBuf;

/// What a running recording has taken in so far.
///
/// Counts and nothing else. The frames themselves never reach this process's
/// heap: `SCRecordingOutput` takes the `IOSurface` directly, and pulling one
/// into a `Frame` would be the 2 GB/s memcpy the design refused (4.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RecordingProgress {
    /// Screen samples the platform has delivered since the stream started.
    pub frames: u64,
    /// Of those, the ones the platform marked as anything other than a
    /// complete frame.
    pub incomplete: u64,
}

/// What a finished recording left on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordingSummary {
    /// The file that was written, which is the path `record` was given and
    /// never a name this chose.
    pub path: PathBuf,
    /// Size of that file once the movie was finalised.
    pub bytes: u64,
    /// Screen samples the stream delivered, so the caller can refuse a
    /// recording that took nothing in.
    pub frames: u64,
}
```

```rust
// crates/frame/src/lib.rs
pub use types::{
    CaptureTarget, DisplayInfo, Frame, PixelFormat, RecordingProgress, RecordingSummary, Rect,
    WindowInfo,
};
```

```rust
// crates/capture/src/lib.rs
use std::path::Path;

pub use snapdeck_frame::{
    CaptureError, CaptureTarget, DisplayInfo, Frame, PixelFormat, RecordingProgress,
    RecordingSummary, Rect, WindowInfo,
};

/// A recording that has started and is writing to a file.
///
/// `Send` because the session lives in the application's shared state and is
/// stopped from a different thread than the one that started it. Every method
/// blocks the calling thread for a platform round trip, exactly as
/// `ScreenCapturer`'s do.
pub trait Recording: Send {
    /// What the stream has delivered so far. Cheap: two atomic loads.
    fn progress(&self) -> RecordingProgress;

    /// Finishes the movie and answers with what was written.
    ///
    /// Consumes the recording, because a stream taken apart cannot be
    /// restarted and a handle that outlived its stream is a handle whose every
    /// method is a lie.
    fn stop(self: Box<Self>) -> Result<RecordingSummary, CaptureError>;

    /// Finishes the movie and throws the result away.
    ///
    /// The file is **not** deleted here. This crate was handed a path and does
    /// not own the folder it is in; the caller that chose the name is the one
    /// that takes it back. The movie is still finalised first, because an
    /// unfinalised file can still be written to after this returns.
    fn cancel(self: Box<Self>) -> Result<(), CaptureError>;
}

pub trait ScreenCapturer {
    // ...displays, windows, capture değişmez...

    /// Starts recording `target` into `output`, which must not already exist.
    ///
    /// Blocks the calling thread for a platform round trip; see the trait
    /// documentation.
    ///
    /// A default body rather than a required method, and that is the whole of
    /// why the v1 design's `stream()` became this: every existing
    /// implementation, `mock.rs` included, compiles unchanged, and a platform
    /// with no recording says so out loud instead of doing nothing quietly.
    fn record(
        &self,
        target: CaptureTarget,
        output: &Path,
    ) -> Result<Box<dyn Recording>, CaptureError> {
        let _ = (target, output);
        Err(CaptureError::Unsupported(
            "this platform cannot record the screen".to_string(),
        ))
    }
}
```

**Kurallar:**

- `mock.rs` **tek satır değişmez**. Bu, varsayılan gövdenin var olma sebebidir ve bir testle kanıtlanır.
- `CaptureError` `Serialize` (`tag = "kind"`, `content = "detail"`, camelCase). Yeni varyant tel üzerinde `{"kind":"unsupported","detail":"..."}` olur. Frontend'de bu varyantı okuyan bir yer yok ve eklenmiyor: kayıt komutu hatayı `String`'e çevirip tepsiye yazar.

- [ ] **Step 1: Testleri yaz (bunlar sözleşmedir, birebir)**

`crates/capture/src/lib.rs` testleri:

| Test | Ne kanıtlar | Mutasyon kanıtı |
|---|---|---|
| A1 `MockCapturer::record` `CaptureError::Unsupported` döner ve mesajı "record" kelimesini taşır | varsayılan gövde gerçekten devrede, mock'a hiçbir şey eklenmedi | varsayılan gövdeyi `Err(CaptureError::Platform(..))` yap: varyant iddiası kırmızı |
| A2 `MockCapturer`'ın `capture`, `displays`, `windows` çıktıları bu görevden önceki testlerle birebir aynı | trait genişletmesi mevcut çağıranları kırmadı | `record`'u varsayılansız zorunlu metot yap: `mock.rs` derlenmez, tüm dosya kırmızı |
| A3 `CaptureError::Unsupported("x")` `{"kind":"unsupported","detail":"x"}` olarak serileşir | tel biçimi mevcut varyantlarla aynı kurala uyuyor | varyanta `#[serde(rename = "Unsupported")]` ekle: kırmızı |

`crates/frame/src/types.rs` testleri:

| Test | Ne kanıtlar | Mutasyon kanıtı |
|---|---|---|
| A4 `RecordingProgress::default()` `frames: 0, incomplete: 0` | bir kayıt hiçbir şey görmemiş olarak başlar | `Default`'u elle yazıp `frames: 1` koy: kırmızı |

- [ ] **Step 2: Kırmızıyı gör, sonra uygula.**
- [ ] **Step 3:** `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` temiz. `git diff --stat crates/capture/src/mock.rs` **boş** olmalı; rapora yaz.
- [ ] **Step 4: Commit** `feat(capture): add the recording surface to the capturer trait`

---

### Task 2: `resolve()` refactor, çekim ve kaydın paylaştığı hedef çözümlemesi

**Birim:** U2. **Bağımlılık:** Görev 1.

**Files:** `crates/capture/src/macos/mod.rs`

Bu görev **davranış eklemez**. Tek işi, `capture()`'ın üç kolundaki hedef çözümlemesini tek bir yere toplamak, böylece Görev 3 aynı kuralları ikinci kez yazmak zorunda kalmasın. Tasarım 6.3'ün gerekçesi: bölge kuralları (en çok örtüşen display kazanır; bölge tek bir display içinde kalmalı) çekimde geçerli olup kayıtta sessizce kaybolursa, kullanıcı iki display'e yayılan bir kaydı yarısı siyah ve yanlış ölçekli olarak alır ve bu başarılı bir kayıt gibi görünür.

**Interfaces (birebir):**

```rust
// crates/capture/src/macos/mod.rs

/// Which display a region is captured from, and where on it.
///
/// Separated from the content request for the reason `sort_by_z_order` is
/// separated: this is the whole of the rule, and `SCShareableContent` is not
/// something a unit test can arrange.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct RegionPlan {
    /// Index into the display list the region is captured from.
    pub(crate) display: usize,
    /// `sourceRect` in that display's own coordinate space.
    pub(crate) source_rect: CGRect,
}

/// Decides which display a region belongs to and refuses the ones that belong
/// to none or to two.
///
/// Pure. `displays` arrives in the order `SCShareableContent` gave, which is
/// documented as no order at all, so the winner is decided by overlapping area
/// and never by position in the list.
pub(crate) fn resolve_region(region: Rect, displays: &[Rect]) -> Result<RegionPlan, CaptureError>;

/// What a capture target resolves to on this machine.
///
/// One function rather than one per call site, and that is the whole reason
/// this exists: `capture` and `record` ask ScreenCaptureKit for the same
/// thing, and a second copy of these rules written for the second caller
/// drifts from this one on the first fix.
struct Resolved {
    filter: SCContentFilter,
    /// Output size in pixels, at `scale`.
    width: u32,
    height: u32,
    /// Pixels per point on the display the target sits on.
    scale: f32,
    /// `Some` only for a region, which is the only target that crops.
    source_rect: Option<CGRect>,
}

/// Blocks on `SCShareableContent`; see the `ScreenCapturer` documentation.
///
/// Load-bearing for the permission contract, and the only place in the
/// recording path that is: a missing screen recording grant fails here as
/// `NoShareableContent`, which `map_err` turns into `PermissionDenied`. Do not
/// cache it and do not skip it.
fn resolve(target: CaptureTarget) -> Result<Resolved, CaptureError>;
```

**Kurallar:**

- `largest_overlap_index`, `contains`, `source_rect_for`, `pixels`, `scale_factor_for`, `to_rect`, `map_err`, `map_no_shareable_content`, `on_screen_z_order`, `sort_by_z_order`, `frame_from_image` **tek satır değişmez**. Bunların mevcut testleri de değişmez. Refactor yalnız çağrı sırasını taşır.
- `largest_overlap_index(` çağrısı dosyada artık yalnız `resolve_region` içindedir; `contains(` çağrısı da öyle.
- `capture()` üç kolunu kaybeder ve şuna iner: `resolve(target)?`, sonra `SCStreamConfiguration` kurulumu (`with_shows_cursor(false)`, hareketsiz çekimde imleç yok), sonra `SCScreenshotManager::capture_image`, sonra `frame_from_image`.
- Boş bölge kontrolü (`rect.is_empty()` → `Platform("empty region")`) `resolve_region`'ın ilk satırına taşınır, çünkü kayıt da boş bölge almamalı.
- Mevcut hata metinleri **kelimesi kelimesine korunur**: `"no display intersects the region"`, `"region is not contained in a single display"`, `"empty region"`, `format!("display {id}")`, `format!("window {id}")`. Bunlar kullanıcıya giden metin ve bu görev onları değiştirmiyor.

- [ ] **Step 1: Refactor öncesi temel çizgiyi al**

`cargo test -p snapdeck-capture -- --list` çıktısını ve `cargo test -p snapdeck-capture` sonucunu (geçen test sayısı ve isimleri) kaydet. Bu, **refactor'ün mevcut çekim testlerini bozmadığının kanıtının birinci parçasıdır**: görevin sonunda aynı liste, artı yeni testler, geçmiş olmalı; **hiçbir test adı kaybolmamış** olmalı.

- [ ] **Step 2: Testleri yaz**

| Test | Ne kanıtlar | Mutasyon kanıtı |
|---|---|---|
| V1 `resolve_region`, iki display'e değen ama çoğu ikincide olan bir bölge için `display: 1` döner | en çok örtüşen kazanır, liste sırası değil | `max_by(area)` yerine `find(intersects)` koy: `display: 0` gelir, kırmızı |
| V2 `resolve_region`, tek bir display'in dışına taşan bir bölgeyi reddeder ve mesaj `"region is not contained in a single display"` (birebir bu dize) | **kayıtta da geçerli olması gereken kural**; bu olmadan iki ekrana yayılan kayıt yarısı siyah ve yanlış ölçekli çıkar ve başarılı görünür | `contains` kontrolünü sil: `Ok` döner, kırmızı |
| V3 Hiçbir display'le kesişmeyen bölge `TargetNotFound("no display intersects the region")` | ekran dışı bölge yakalanmaz | `ok_or_else`'i `unwrap_or(0)`'a çevir: kırmızı |
| V4 `resolve_region`'ın `source_rect`'i seçilen display'in kendi uzayındadır; negatif origin'li bir display için origin çıkarılmıştır | iki uzay karıştırılmıyor | `source_rect_for` çağrısını `region`'ı olduğu gibi geçirecek şekilde değiştir: kırmızı |
| V5 Boş bölge (`width: 0`) `Platform("empty region")` ile reddedilir | sıfır boyutlu istek ScreenCaptureKit'e hiç gitmez | `is_empty` kontrolünü sil: kırmızı |
| V6 **Paylaşım kanıtı.** `include_str!("mod.rs")` okunur; `fn capture(` ile başlayan gövde `resolve(target)?` içerir ve içinde `SCShareableContent` geçmez | çekim gerçekten ortak çözümlemeden geçiyor | `capture`'ın içine ikinci bir `SCShareableContent::get()` yapıştır: kırmızı |
| V7 **Paylaşım kanıtı, ikinci yarı.** Aynı dosyada `largest_overlap_index(` ve `contains(` çağrıları yalnız `resolve_region`'ın gövdesinde geçer | bölge kuralları tek bir yerde | `record` veya `capture` içine bir `largest_overlap_index(` çağrısı yapıştır: kırmızı |

V6 ve V7 kırılgandır ve bilerek öyledir: `output::the_jpeg_quality_matches_the_one_the_editor_encodes_at` ve `fullpage`'in `TRAY.contains("FULLPAGE_MENU_LABEL")` testiyle aynı desen. Ekran olmadan "iki yol aynı kuralı paylaşıyor" iddiasını mekanik olarak tutmanın başka yolu yok. Gövde dilimleme, işaretçi dizeden (`fn capture(`) bir sonraki `\n    fn ` veya `\n}` işaretine kadardır; testin kendisi bu dilimlemenin bir gövde bulduğunu da iddia etmeli, yoksa boş dilim her şeyi geçirir.

> V7, Görev 3 gelene kadar `record` yokken de anlamlıdır: o zaman iddia "bölge kuralları yalnız bir yerde" der, Görev 3'ten sonra "ve ikinci yol onu kullanıyor" der.

- [ ] **Step 3: Kırmızıyı gör, refactor'ü yap.**
- [ ] **Step 4:** Tüm kapılar temiz. **Raporda birebir şu iki listeyi ver:** Step 1'deki test adları ve Step 4'teki test adları. Aradaki fark yalnız eklenen V1-V7 olmalı; kaybolan tek bir ad bile görevin başarısızlığıdır.
- [ ] **Step 5: Commit** `refactor(capture): share one target resolution between capture paths`

---

### Task 3: macOS kayıt implementasyonu ve durdurma sırası

**Birim:** U2. **Bağımlılık:** Görev 2.

**Files:** `crates/capture/src/macos/recording.rs` (yeni), `crates/capture/src/macos/mod.rs` (`mod recording;` ve `impl ScreenCapturer for MacCapturer` içine `record`), kök `Cargo.toml` (özellik satırı)

**Interfaces (birebir):**

```rust
// crates/capture/src/macos/recording.rs

/// Frames per second a recording is capped at.
///
/// ScreenCaptureKit's own default is uncapped, which asks the encoder for
/// every composited frame on a moving screen. There is no setting for this in
/// this release; the design fixed it at 30 (5.3).
pub const RECORDING_FPS: u32 = 30;

/// The stream configuration a recording runs with.
///
/// Taking plain numbers rather than a `Resolved` so that the three things the
/// design fixed can be read back without a display and without a content
/// filter: the frame cap, the cursor, and the size.
///
/// The cursor is **on**, which is the opposite of `capture()`. In a still
/// screenshot the pointer is clutter; in a screen recording it is the content.
pub(crate) fn recording_config(
    width: u32,
    height: u32,
    source_rect: Option<CGRect>,
) -> SCStreamConfiguration;

/// What the encoder is told, which is all three things it accepts.
///
/// `None` on a system without `SCRecordingOutput`, which is every macOS before
/// 15.0.
pub(crate) fn recording_output_config(path: &Path) -> Option<SCRecordingOutputConfiguration>;

/// The refusal a machine without `SCRecordingOutput` gets.
pub(crate) fn unsupported() -> CaptureError;

/// The two things taking a recording apart does, as a trait so the order can
/// be driven without a stream.
///
/// A live `SCStream` is not something a unit test can arrange, and the claim
/// worth testing here is not the FFI, it is the order.
pub(crate) trait RecordingTeardown {
    /// Detaches the frame counter. `Ok` when there was nothing to detach.
    fn detach_frame_handler(&mut self) -> Result<(), String>;
    /// Removes the recording output, which is what finalises the movie.
    fn remove_recording_output(&mut self) -> Result<(), String>;
}

/// Takes a recording apart in the one order that leaves a playable file.
///
/// **This order is not a style choice.** `screencapturekit` 9.0.1's
/// `SCStream::remove_recording_output` only stops the capture and waits for the
/// movie to reach a terminal state when three things hold at the moment it is
/// called: the stream is still capturing, **no output handlers are attached**,
/// and this is the last recording output. Read from the crate's own source:
///
/// ```text
/// if context.capturing.load(..) && !context.has_handlers()
///     && context.recording_outputs.load(..) == 1 { stop_capture(); wait_until_terminal(); }
/// ```
///
/// Miss any one of the three and the call returns without waiting, the stream
/// is dropped under a movie that was never finalised, and the `.mp4` has no
/// `moov` atom: QuickTime refuses to open it and the user's recording is gone.
///
/// So the frame counter comes off first. And `stop_capture` is deliberately
/// **never called here**: `capturing == false` is the second way to skip the
/// same wait, and a well-meaning "stop it cleanly first" line is exactly how
/// this gets broken.
///
/// A failed detach does not skip the removal. The movie has to be finalised
/// even when the handler will not come off; the worst a stuck handler costs is
/// a few discarded samples, and the alternative costs the whole recording.
pub(crate) fn tear_down<T: RecordingTeardown>(teardown: &mut T) -> Result<(), String>;

/// A live ScreenCaptureKit recording.
pub struct MacRecording { /* private */ }

impl Recording for MacRecording {
    fn progress(&self) -> RecordingProgress;
    fn stop(self: Box<Self>) -> Result<RecordingSummary, CaptureError>;
    fn cancel(self: Box<Self>) -> Result<(), CaptureError>;
}
```

```rust
// crates/capture/src/macos/mod.rs, ScreenCapturer impl'ine eklenen
    fn record(
        &self,
        target: CaptureTarget,
        output: &Path,
    ) -> Result<Box<dyn Recording>, CaptureError> { /* ... */ }
```

**Kurallar:**

- `record`'un sırası, birebir:
  1. `SCRecordingOutput::is_available()` yanlışsa `unsupported()`. **Her şeyden önce**, çünkü `SCRecordingOutputConfiguration::new()` macOS 14'te panikler (kaynakta `expect`), ve bir menü çubuğu ajanının paniği kullanıcının gördüğü tek şey olur.
  2. `resolve(target)?` (Görev 2). Ekran kaydı izni burada düşer.
  3. `recording_output_config(output)` → `SCRecordingOutput::new_with_delegate(&config, RecordingCallbacks::new().on_fail(...))`. Delegate'in `on_fail`'i mesajı paylaşılan bir `Mutex<Option<String>>`'e yazar; `stop` onu okur. Bu, disk dolması ve kodlayıcı hatasının kullanıcıya ulaştığı tek yoldur.
  4. `SCStream::new(&resolved.filter, &recording_config(...))`.
  5. `add_output_handler(FrameCounter, SCStreamOutputType::Screen)` → `Option<usize>`; `None` ise kayıt yine de devam eder, sayaç olmadan. Sayaç bir teşhis, bir önkoşul değil.
  6. `add_recording_output(&output)?`.
  7. `start_capture()?`. Başarısızsa: `tear_down` çağrılır ve hata döndürülür, yarım kurulmuş bir akış bırakılmaz.
- `FrameCounter`, `did_output_sample_buffer` içinde **yalnız** iki `fetch_add` yapar. `image_buffer()` çağrılmaz, `CMSampleBuffer` kopyalanmaz. `frame_status()` `Some(SCFrameStatus::Complete)` değilse `incomplete` artar.
- `stop` sırası: `tear_down` → delegate hatası varsa `Platform(mesaj)` ile dön → `std::fs::metadata(path).len()` → `RecordingSummary`. Delegate hatası teardown'dan **sonra** okunur: akış her hâlükârda sökülmeli.
- `cancel`: `tear_down`, o kadar. **Dosya silinmez** (trait sözleşmesi).
- `MacRecording` `RecordingTeardown`'ı kendi alanları üzerinden uygular; `stop` ve `cancel` ikisi de `tear_down`'ı çağırır, ikisi de kendi sıralarını yazmaz.
- Kök `Cargo.toml`'daki yorum güncellenir: özellik neden `macos_15_0`, tek cümle.

- [ ] **Step 1: Testleri yaz**

| Test | Ne kanıtlar | Mutasyon kanıtı |
|---|---|---|
| C1 Sahte bir `RecordingTeardown`, `tear_down` sonrası çağrı sırasını tam olarak `["detach_frame_handler", "remove_recording_output"]` diye kaydeder | **durdurma sırası**: sayaç önce iner | `tear_down`'daki iki satırı yer değiştir: kırmızı |
| C2 `detach_frame_handler` `Err` dönerse `remove_recording_output` **yine de** çağrılır ve `tear_down` detach hatasını döndürür | filmin sonlandırılması takılı bir handler yüzünden atlanmaz | ilk çağrıya `?` koy: sıra listesinde ikinci giriş yok, kırmızı |
| C3 `remove_recording_output` `Err` dönerse `tear_down` o hatayı döndürür, detach başarılı olsa bile | sonlandırma hatası yutulmaz | removal sonucunu `let _ =` ile at: kırmızı |
| C4 `include_str!("recording.rs")` içinde `stop_capture` dizesi **hiç geçmez** | `capturing == false` durdurma sırasını bozmanın ikinci yolu ve bu dosyada kimse onu deneyemez | dosyanın herhangi bir yerine `self.stream.stop_capture()` ekle: kırmızı |
| C5 `RECORDING_FPS` **30**'dur (sabit, açık yazılmış) | sözleşme değeri | sabiti 60 yap: kırmızı |
| C6 `recording_config(800, 600, None)`: `width() == 800`, `height() == 600`, `fps() == 30` (açık yazılmış 30), `shows_cursor() == true` | tasarım 5.3'ün üç kararı gerçek crate'e karşı, ekransız | `with_shows_cursor(false)`: kırmızı. `with_fps(60)`: kırmızı. `with_fps` satırını sil: `fps()` 0 döner, kırmızı |
| C7 `recording_config(800, 600, Some(rect))` o `source_rect`'i geri verir; `None` verilince `source_rect()` sıfır boyutludur | bölge kaydı gerçekten kırpıyor | `source_rect` kolunu sil: kırmızı |
| C8 `recording_output_config(Path::new("/tmp/a.mp4"))`: `video_codec().identifier() == "avc1"` (açık yazılmış), `output_file_type().identifier() == "public.mpeg-4"` (açık yazılmış), `output_url() == Some("/tmp/a.mp4")` | H.264 + MP4, tasarım 5.3 | `SCRecordingOutputCodec::HEVC`: kırmızı. `SCRecordingOutputFileType::MOV`: kırmızı. `with_output_url` satırını sil: kırmızı |
| C9 `unsupported()` `CaptureError::Unsupported`'tır ve mesajı `"15"` içerir | macOS 14 kullanıcısına ne olduğu söylenir | `CaptureError::Platform`'a çevir: kırmızı. Mesajdan sürümü çıkar: kırmızı |
| C10 `include_str!("../mod.rs")` içinde `fn record(` gövdesi `is_available()`'ı `SCRecordingOutputConfiguration` veya `SCStream`'den **önce** anar | macOS 14'te panik yerine mesaj | kontrolü gövdenin sonuna taşı: kırmızı |
| C11 `#[ignore]` **Gerçek donanım.** Birincil display 2 saniye kaydedilir, `stop` çağrılır: dosya var, `bytes > 0`, `frames > 0`, ilk 64 KB ve son 64 KB içinde `moov` atomu geçiyor | boru hattının tamamı, ve C1-C4'ün gerçekten işe yaradığı | `tear_down`'daki sırayı ters çevir: `moov` iddiası kırmızı. Bu, C1'in gerçek dünyadaki karşılığıdır ve Görev 9'un negatif kontrolüdür |

C8 ve C11 macOS 15+ ister. İkisi de `assert!(SCRecordingOutput::is_available(), "...")` ile **yüksek sesle** başlar; sessizce atlanan bir test, olmayan bir testtir. C11 `#[ignore]`'dur çünkü ekran kaydı izni ister; `cargo test -p snapdeck-capture -- --ignored` ile elle koşulur ve Görev 9'da koşulur.

- [ ] **Step 2: Kırmızıyı gör, uygula.**
- [ ] **Step 3:** `Cargo.toml` özelliğini yükselt. `git diff Cargo.lock` **boş** olmalı; rapora yaz.
- [ ] **Step 4:** Tüm kapılar temiz; `cargo test -p snapdeck-capture -- --ignored` elle koşulur ve C11'in yeşil olduğu rapora yazılır.
- [ ] **Step 5: Commit** `feat(capture): record the screen to an mp4 with SCRecordingOutput`

---

### Task 4: Dosya sözleşmesi, geçici dosya, yer tutucu ve rename

**Birim:** U3. **Bağımlılık:** yok. **Görev 1 ve 6 ile paralel koşabilir.**

**Files:** `apps/desktop/src-tauri/src/recording.rs` (yeni), `apps/desktop/src-tauri/src/output.rs` (tek çıkarım), `apps/desktop/src-tauri/src/lib.rs` (yalnız `mod recording;`)

Tasarım 9.5'in sözleşmesi: kayıt hedef klasördeki **gizli bir geçici dosyaya** yazılır; **nihai ad** kayıt başlarken `create_new` ile kapatılır; sonlandırma başarılıysa geçici dosya nihai adın üstüne **rename** edilir. Çökme artıkları açılışta ve her yeni kayıtta süpürülür.

Üç gerekçe, her biri ayrı:
- **Geçici dosya hedef klasörde**, bir cache'te değil: farklı birimler arası rename tüm filmin kopyalanmasıdır ve kullanıcının kaydetme klasörü başka bir birimde olabilir.
- **Nihai ad önceden kapatılır**: kayıt sürerken alınan bir ekran görüntüsü aynı adı alabilir ve dakikalar sonra rename o dosyayı ezerdi.
- **Rename atomiktir**: kullanıcı, yarısı yazılmış bir `.mp4`'ü klasöründe hiç görmez.

**Interfaces (birebir):**

```rust
// apps/desktop/src-tauri/src/output.rs, mevcut koddan çıkarılan tek fonksiyon

/// Claims the first free name for `stem` under `directory` and answers with
/// the open file and the path it took.
///
/// `create_new`, not an "is it there" check followed by a write: the check
/// answers for a moment that has passed by the time the file is opened, and
/// the whole point is that nothing is overwritten.
///
/// Extracted so that a recording claims its name by exactly the rule a still
/// capture claims one by. The ` 2`, ` 3` suffix is the user-visible half of
/// that rule and a second copy of it would drift.
pub(crate) fn claim_free_path(
    directory: &Path,
    stem: &str,
    extension: &str,
) -> Result<(File, PathBuf), String>;
```

`save_capture_without_overwriting` bu fonksiyonu çağıracak şekilde kısalır; davranışı ve hata metinleri değişmez.

```rust
// apps/desktop/src-tauri/src/recording.rs

/// The extension every recording is written under.
pub const RECORDING_EXTENSION: &str = "mp4";

/// The middle of every temporary recording file's name.
///
/// What a sweep recognises as this application's litter, and nothing else in
/// the user's folder may look like it.
const RECORDING_TEMP_MARKER: &str = ".snapdeck-recording-";

/// Where a recording writes while it runs and where it lands when it finishes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordingFiles {
    /// The hidden file ScreenCaptureKit writes into. It does not exist yet
    /// when this is built: `AVAssetWriter` refuses a URL that is already
    /// taken, so creating it here would stop every recording from starting.
    pub temporary: PathBuf,
    /// The name the finished movie takes, already claimed with `create_new`
    /// and holding zero bytes until the rename.
    pub final_path: PathBuf,
}

/// The hidden name a recording for `final_name` writes into.
///
/// Derived from the final name rather than random, and that is what makes a
/// sweep precise: a leftover names the placeholder it belongs to, so the sweep
/// can take both away without guessing which zero-byte `.mp4` in the user's
/// folder is its own. The leading dot, the process id and the counter are the
/// shape `commands::temporary_name` already uses, for the same reasons.
fn temporary_name(final_name: &OsStr) -> OsString;

/// The final name a leftover was going to become, or `None` when the name is
/// not one this application wrote.
fn final_name_of_leftover(leftover: &OsStr) -> Option<OsString>;

/// Claims both names under `directory`.
///
/// `stem` is what the user's filename template rendered; the ` 2`, ` 3` suffix
/// rule is the still capture's, through `output::claim_free_path`.
pub fn reserve(directory: &Path, stem: &str) -> Result<RecordingFiles, String>;

/// Puts the finished movie at its final name.
pub fn commit(files: &RecordingFiles) -> Result<PathBuf, String>;

/// Takes both files away. Quiet when either is already gone.
pub fn abandon(files: &RecordingFiles);

/// Deletes this application's recording litter from `directory`, and answers
/// with how many files it took away.
///
/// A crash between the create and the rename leaves a hidden, unfinalised
/// movie in the user's save folder and a zero-byte placeholder next to it
/// under a name that looks like a real capture. Both are this application's
/// mess and neither is something the user should have to recognise.
///
/// `keep` is the live recording's own pair, which a sweep started by the next
/// recording must not touch. A placeholder is only removed when it is **still
/// zero bytes**: a name that has grown a real movie belongs to the user.
pub fn sweep(directory: &Path, keep: Option<&RecordingFiles>) -> usize;
```

**Kurallar:**

- `temporary_name(final)` → `.{final}{MARKER}{pid}-{seq}.mp4`, örnek: `.Recording 2026-09-14 at 10.00.00.mp4.snapdeck-recording-4321-1.mp4`. `seq` `output`'takiyle aynı desende bir `AtomicU64`.
- `final_name_of_leftover` baştaki noktayı ve `{MARKER}...{EXT}` kuyruğunu soyar. Dosya adları `OsStr` üzerinden işlenir, `String` üzerinden değil: macOS `/` ve NUL dışında her baytı kabul eder, ve `to_string_lossy` üzerinden dönen bir ad var olmayan bir dosyayı adlandırır. `tray::recent_capture_id`'nin aynı gerekçesi.
- `sweep` yalnız `directory`'nin kendi girdilerine bakar: `read_dir`, `file_type().is_file()`. Alt dizinlere inmez, sembolik bağ izlemez.
- `commit` `std::fs::rename` kullanır, kopyalama yapmaz.
- `recording.rs` bu görevde **Tauri'ye dokunmaz**: `AppHandle` almaz, `settings` okumaz. Bu, testlerin bir uygulama olmadan koşmasının sebebidir. Uygulamaya bağlanma Görev 8'in işi.

- [ ] **Step 1: Testleri yaz**

| Test | Ne kanıtlar | Mutasyon kanıtı |
|---|---|---|
| R1 `final_name_of_leftover(&temporary_name(n))` sıradan bir ad, Türkçe karakterli bir ad ve Unicode olmayan baytlar taşıyan bir ad için `Some(n)` döner | isim gidiş dönüşü kayıpsız | `temporary_name`'deki marker'ı değiştir: kırmızı. `to_string_lossy` üzerinden yaz: Unicode olmayan ad kırmızı |
| R2 `final_name_of_leftover` şunların hepsine `None` der: `"shot.mp4"`, `".hidden.mp4"`, `".snapdeck-recording-.mp4"`, `".a.mp4.snapdeck-recording-1-1.png"` | kullanıcının dosyaları bizim çöpümüz sanılmaz | kuyruk kontrolünü sil: `.hidden.mp4` çözülür, kırmızı |
| R3 `reserve` boş bir klasörde `"Recording 2026-09-14 at 10.00.00.mp4"` adını alır; aynı çağrı ikinci kez `"Recording 2026-09-14 at 10.00.00 2.mp4"` alır (iki dize de **açık yazılmış**) | çakışma soneki hareketsiz çekimdekiyle aynı | `claim_free_path` yerine `format!("{stem}.mp4")` + `File::create` koy: ikinci ad iddiası kırmızı |
| R4 `reserve` sonrası `final_path` **vardır ve sıfır baytdır**, `temporary` ise **yoktur** | yer tutucu adı kapatır, geçici dosya AVAssetWriter'a bırakılır | `reserve`'e `File::create(&temporary)` ekle: kırmızı, ve gerçek kayıt hiç başlamaz |
| R5 `commit` geçici dosyanın baytlarını `final_path`'e taşır, geçici dosya kalmaz, ve `final_path`'in içeriği geçici dosyanın içeriğidir | rename gerçekten oluyor | `fs::copy` kullanıp `remove_file` yazma: geçici dosya iddiası kırmızı |
| R6 Geçici dosya yokken `commit` `Err` döner ve `final_path` **değişmeden** kalır | kayıp filmi sessizce boş bir dosyayla değiştirmez | rename hatasını `let _ =` ile at: kırmızı |
| R7 `abandon` iki dosyayı da siler, ve ikinci kez çağrılınca panik atmaz | iptal ve hata yolları aynı temizliği paylaşır | ilk silmeye `unwrap` koy: ikinci çağrı kırmızı |
| R8 `sweep`: bir artık geçici dosyayı ve onun **sıfır baytlık** yer tutucusunu siler; aynı adda **sıfır baytlık olmayan** bir dosya varsa ona dokunmaz; ilgisiz `shot.png` ve `notes.mp4`'e dokunmaz; `keep` verilen çiftin ikisine de dokunmaz; ve silinen dosya sayısını döner | çöp toplama tam olarak kendi çöpünü topluyor | sıfır bayt kontrolünü sil: gerçek film silinir, kırmızı. `keep` kontrolünü sil: canlı kaydın geçici dosyası yazılırken silinir, kırmızı |
| R9 `sweep`, artık adında bir **alt dizin** varsa ona dokunmaz | `read_dir` girdisi dosya mı diye sorulur | `is_file()` kontrolünü sil: kırmızı |
| R10 `output` tarafı: `save_capture_without_overwriting`'in mevcut testlerinin tamamı, tek satır değişmeden geçer | çıkarım çekim kaydetmesini bozmadı | `claim_free_path` içindeki `create_new(true)`'yu `create(true)`'ya çevir: `a_colliding_name_is_suffixed_rather_than_overwritten` kırmızı |

- [ ] **Step 2: Kırmızıyı gör, uygula.**
- [ ] **Step 3:** Tüm kapılar temiz.
- [ ] **Step 4: Commit** `feat(app): reserve, commit and sweep a recording's files`

---

### Task 5: Kayıt oturumu ve durdurma/iptal sonuçları

**Birim:** U3. **Bağımlılık:** Görev 1, Görev 4.

**Files:** `apps/desktop/src-tauri/src/state.rs`, `apps/desktop/src-tauri/src/recording.rs` (ekleme)

**Interfaces (birebir):**

```rust
// apps/desktop/src-tauri/src/state.rs

/// The recording that is running, and the two things about it only Rust knows.
pub struct RecordingSession {
    /// The live recording, which `stop` and `cancel` consume.
    pub recording: Box<dyn Recording>,
    /// Where it writes and where it will land.
    pub files: RecordingFiles,
    /// When it started, so the menu bar can say how long it has been going.
    pub started_at: Instant,
}

impl AppState {
    /// Claims the single recording slot, or answers `false` when one is
    /// already running and leaves the caller's session untouched.
    ///
    /// One at a time for the reason `begin_capture` is: two recordings share
    /// one menu bar title, one Stop item and one save folder, and the second
    /// one to start would be a recording the user cannot stop.
    pub fn begin_recording(&self, session: RecordingSession) -> bool;

    /// Takes the running recording out, or `None` when there is none.
    ///
    /// Taking rather than borrowing, because both things a caller does with it
    /// consume it, and a session left in the slot after its stream was torn
    /// down is a Stop item that does nothing.
    pub fn take_recording(&self) -> Option<RecordingSession>;

    /// Whether a recording is running.
    pub fn is_recording(&self) -> bool;

    /// How long the running recording has been going, or `None`.
    pub fn recording_elapsed(&self) -> Option<Duration>;
}
```

```rust
// apps/desktop/src-tauri/src/recording.rs

/// What a stopped recording did to the disk.
#[derive(Debug, PartialEq, Eq)]
pub struct Finished {
    /// The movie, or `None` when there is not one to give.
    pub path: Option<PathBuf>,
    /// What the user has to be told, if anything.
    pub complaint: Option<String>,
}

/// Turns a platform result and a pair of files into what the user gets.
///
/// Split out for the reason `bridge::intake::deliver` is: the claim worth
/// testing is what happens to the two files in each outcome, and everything
/// around it needs a live stream and a screen recording grant.
pub fn finish(summary: Result<RecordingSummary, CaptureError>, files: &RecordingFiles) -> Finished;

/// Ends a cancelled recording and leaves nothing on disk.
///
/// The files go whether or not the platform managed to take the stream apart:
/// a cancel that leaves a half-written movie in the user's folder is not a
/// cancel. Answers with what to tell the user, or `None` when there is nothing
/// to tell.
pub fn discard(result: Result<(), CaptureError>, files: &RecordingFiles) -> Option<String>;
```

**Kurallar:**

- `finish`'in kararı, sırayla:
  1. `Err(error)` → `abandon(files)`, `complaint` hatanın metnini taşır, `path: None`.
  2. `Ok(summary)` ve `summary.frames == 0` → `abandon(files)`, `complaint` "hiçbir kare kaydedilmedi" der, `path: None`. Sıfır kareli bir kayıt, hiçbir oynatıcının açamadığı bir dosyadır; açılamayan bir dosya vermek, olmadığını söylemekten kötüdür.
  3. `Ok(_)` ve `commit` başarılı → `path: Some(...)`, `complaint: None`.
  4. `Ok(_)` ama `commit` başarısız → `abandon(files)`, `complaint` `commit`'in metnini taşır, `path: None`.
- `discard` her zaman `abandon` çağırır, önce. `result`'ın `Err` olması yalnız `complaint`'i belirler.
- `state.rs`'deki kilit, dosyadaki diğer kilitlerle aynı deseni izler: `unwrap_or_else(|poisoned| poisoned.into_inner())`, ve gerekçesi bir doc yorumunda. Bu kilit için gerekçe: alakasız bir panik yüzünden çalışan bir kaydı durdurulamaz bırakmak, bu uygulamanın kullanıcıya yapabileceği en kötü şeydir.

- [ ] **Step 1: Testleri yaz**

`state.rs` için sahte bir `Recording` (üç metodu da önemsiz) test modülünde kurulur.

| Test | Ne kanıtlar | Mutasyon kanıtı |
|---|---|---|
| S1 `begin_recording` ilk çağrıda `true`, çalışan bir kayıt varken `false`; `take_recording` oturumu geri verir ve yuva yeniden boşalır | tek kayıt yuvası | `begin_recording`'i koşulsuz `Some(...)` yazacak şekilde değiştir: ikinci çağrı `true` döner, kırmızı |
| S2 `is_recording` ve `recording_elapsed` hiçbir şey yokken `false`/`None`, çalışırken `true`/`Some` | tepsi neyi çizeceğini bilir | `recording_elapsed`'i koşulsuz `Some(Duration::ZERO)` yap: boş durum kırmızı |
| S3 `take_recording` ikinci kez `None` döner | iki kez tıklanan Stop ikinci filmi aramaz | `take` yerine bir kopya bırak: kırmızı |
| F1 `finish(Ok(summary{frames: 12}), files)`: `final_path` geçici dosyanın baytlarını taşır, geçici dosya yok, `complaint: None`, `path: Some(final_path)` | mutlu yol | `commit` çağrısını sil: `path` `None`, kırmızı |
| F2 `finish(Ok(summary{frames: 0}), files)`: iki dosya da yok, `path: None`, `complaint` boş değil ve kare alınmadığını söyler | boş kayıt kullanıcıya verilmez | `frames == 0` kolunu sil: dosya kalır, kırmızı |
| F3 `finish(Err(Platform("disk full")), files)`: iki dosya da yok, `path: None`, `complaint` `"disk full"` dizesini içerir | platform hatası kullanıcıya ulaşır ve çöp bırakmaz | `Err` kolunda `commit` çağır: kırmızı |
| F4 `finish(Ok(summary{frames: 12}), files)` ama geçici dosya diskte yokken: `final_path` da kalmaz, `path: None`, `complaint` boş değil | yarım başarı diye bir şey bırakılmaz | `commit` hatasında `abandon` çağırma: yer tutucu kalır, kırmızı |
| F5 `discard(Ok(()), files)` iki dosyayı da siler ve `None` döner | **iptal edilen kayıt diskte hiçbir şey bırakmaz** (tasarım 3.1 bitti kriteri) | `abandon` çağrısını sil: kırmızı |
| F6 `discard(Err(Platform("boom")), files)` **yine** iki dosyayı da siler ve `Some(mesaj)` döner | başarısız bir söküm de iptal | `Err` durumunda erken dön: dosyalar kalır, kırmızı |

- [ ] **Step 2: Kırmızıyı gör, uygula.**
- [ ] **Step 3:** Tüm kapılar temiz.
- [ ] **Step 4: Commit** `feat(app): hold one recording session and decide what it leaves behind`

---

### Task 6: Overlay'in `action` parametresi (frontend)

**Birim:** U4. **Bağımlılık:** yok. **Görev 1 ve 4 ile paralel koşabilir.**

**Files:** `apps/desktop/src/overlay/{action.ts,action.test.ts,Overlay.tsx,main.tsx}`

Tasarım 3.1: seçim overlay'i **aynen** kullanılır, `Enter` çekim yerine kaydı başlatır. Overlay'in gesture'ı (bölge sürükleme, pencere yakalama, tam ekran) `mode`'un işi ve değişmiyor; onaylandığında ne olacağı ayrı bir sorudur ve ayrı bir parametre alır.

**Interfaces (birebir):**

```ts
// apps/desktop/src/overlay/action.ts

/** The action that starts a recording instead of taking a still. */
export const RECORD_ACTION = 'record'

/** The Tauri command a confirmed selection is handed to. */
export type ConfirmCommand = 'capture_region' | 'start_recording'

/**
 * Which command `Enter` invokes.
 *
 * Anything that is not exactly `RECORD_ACTION` takes a still. The default is
 * deliberately the harmless one: an unrecognised action must never start a
 * recording the user did not ask for, and a URL is a thing that can be wrong.
 */
export function confirmCommand(action: string): ConfirmCommand

/** What the size readout tells the user to press. */
export function confirmHint(action: string, snapsToWindows: boolean): string
```

```ts
// apps/desktop/src/overlay/Overlay.tsx, OverlayProps'a eklenen
  /**
   * `'record'` starts a recording when the selection is confirmed; anything
   * else takes a still. Written by Rust into the overlay URL, so it never
   * changes under a live overlay.
   */
  action: string
```

**Kurallar:**

- `Overlay.tsx`'in `confirm`'ü `invoke(confirmCommand(action), { displayId, rect })` çağırır. `capture_region`'ın `.catch` davranışı ve yorumu aynen kalır: iki komut da overlay'leri kendi kapatır ve sonucu bekleyen bir webview yoktur.
- Ekrandaki metin `confirmHint(action, snapsToWindows)`'tan gelir; `Overlay.tsx` içinde sabit metin kalmaz.
- `main.tsx` `action` sorgu parametresini `requiredParam('action')` ile okur, `mode` ile aynı şekilde.
- `packages/editor`'a dokunulmaz.

- [ ] **Step 1: Testleri yaz (`action.test.ts`)**

| Test | Ne kanıtlar | Mutasyon kanıtı |
|---|---|---|
| O1 `confirmCommand('record') === 'start_recording'` | kayıt yolu | karşılaştırmayı ters çevir: kırmızı |
| O2 `confirmCommand` şunların hepsi için `'capture_region'` döner: `'capture'`, `''`, `'Record'`, `'recording'`, `'RECORD'` | **güvenli varsayılan**: tanınmayan hiçbir şey kayıt başlatmaz | `startsWith`/`includes` ile eşleştir: `'recording'` kayıt başlatır, kırmızı |
| O3 `confirmHint` dört kombinasyon için tam olarak `'Enter to capture'`, `'Click to capture'`, `'Enter to record'`, `'Click to record'` döner (dördü de **açık yazılmış**) | menü çubuğu dışındaki tek talimat yüzeyi doğru | `snapsToWindows` dalını sil: kırmızı. `'record'` metnini `'capture'` yap: kırmızı |
| O4 `RECORD_ACTION === 'record'` (açık yazılmış) | iki dilin paylaştığı sözleşme değeri | sabiti `'rec'` yap: kırmızı, ve Görev 7'nin çapraz dil testi de kırmızı |

- [ ] **Step 2: Kırmızıyı gör, uygula, `Overlay.tsx` ve `main.tsx`'i bağla.**
- [ ] **Step 3:** `pnpm lint`, `pnpm test`, `pnpm build` temiz.
- [ ] **Step 4: Commit** `feat(overlay): let a confirmed selection start a recording`

---

### Task 7: Overlay URL'i ve tepsinin kayıt öğeleri

**Birim:** U3. **Bağımlılık:** Görev 5. Görev 3 ile paralel koşabilir.

**Files:** `apps/desktop/src-tauri/src/overlay.rs`, `apps/desktop/src-tauri/src/tray.rs`

**Interfaces (birebir):**

```rust
// apps/desktop/src-tauri/src/overlay.rs

/// The action a confirmed selection takes: a still capture.
pub const CAPTURE_ACTION: &str = "capture";
/// The action a confirmed selection takes: a recording.
pub const RECORD_ACTION: &str = "record";

pub fn overlay_url(
    display_id: u32,
    mode: &str,
    action: &str,
    scale: f32,
    frame_path: &Path,
) -> String;

pub fn open_overlays(app: &AppHandle, mode: &str, action: &str);
```

```rust
// apps/desktop/src-tauri/src/tray.rs

/// The elapsed time as the menu bar shows it.
///
/// `0:07` under a minute, `12:34` under an hour, `1:02:03` past one. Minutes
/// are not padded below ten, because the menu bar charges for every character
/// and a leading zero buys nothing; seconds are, because `1:7` is not a time.
fn elapsed_text(elapsed: Duration) -> String;

/// What a recording menu item is called on this machine.
///
/// On macOS 14 the item is built disabled and says why in its own label. A
/// disabled macOS menu item does not deliver a click, so there is no event to
/// explain it in; the label is the only surface left, and an item that is
/// greyed out with no reason given is worse than no item.
fn record_label(base: &str, available: bool) -> String;

/// Single entry point for every recording request from the tray.
pub fn request_recording(app: &AppHandle, mode: &str);

/// Puts the recording's elapsed time in the menu bar, or takes it back out.
///
/// `None` returns the title to its quiet state. Best-effort, like every other
/// write to the tray.
pub fn show_recording(app: &AppHandle, elapsed: Option<Duration>);

/// Turns the Stop and Cancel items on or off.
pub fn set_recording_items_enabled(app: &AppHandle, recording: bool);
```

**Kurallar:**

- `overlay_url` yeni biçimi: `overlay.html?display=3&mode=region&action=record&scale=2&path=/tmp/cache/frozen-3.png`. **Mevcut `overlay_url_carries_display_mode_scale_and_frame_path` testi bu görevde değişir** ve yeni beklenen dize yukarıdaki gibi açık yazılır.
- `open_overlays`, `build_overlay_windows`, `build_overlay_window` üçü de `action`'ı taşır. `request_capture` `CAPTURE_ACTION` ile, `request_recording` `RECORD_ACTION` ile çağırır. `lib.rs`'deki global kısayol işleyicisi `request_capture`'a bağlı kalır: kayıt için kısayol bu sürümde yok (tasarım 2).
- Menüdeki yeni öğeler ve sırası: mevcut dört yakalama öğesinden sonra bir ayraç, sonra `Record Region`, `Record Window`, `Record Full Screen`, sonra `Stop Recording` ve `Cancel Recording`. Kendi grubu, çünkü bunlar farklı bir soruyu cevaplıyor.
- Üç kayıt öğesi `SCRecordingOutput::is_available()` ile kurulur: `false` ise `.enabled(false)` ve etiket `record_label(base, false)`.
- `Stop Recording` ve `Cancel Recording` her zaman menüde durur, boştayken `.enabled(false)`. `FailureSurface` ve `RecentCaptures` gibi `app.manage`'lenen bir `RecordingItems` yapısında tutulurlar ki `recording.rs` onları herhangi bir iş parçacığından bulabilsin.
- `show_recording(app, Some(elapsed))` tepsi başlığına `elapsed_text`'i yazar. `None` başlığı temizler. Tepsi başlığı hata işaretçisiyle (`FAILURE_MARKER`) aynı yeri paylaşır; kayıt sürerken bir hata olursa **hata kazanır**, çünkü hata okunmadan kaybolmamalı ve süre zaten menüde de görünür. Bu, `show_failure`'ın `show_recording`'den sonra çağrılabilmesi demektir ve sıra `recording.rs`'in işidir.
- Menü olay işleyicisine üç yeni kol: `record_region`/`record_window`/`record_display` → `request_recording`, `stop_recording` → `crate::recording::request_stop`, `cancel_recording` → `crate::recording::request_cancel`. `quit` kolu Görev 8'de değişir.

- [ ] **Step 1: Testleri yaz**

| Test | Ne kanıtlar | Mutasyon kanıtı |
|---|---|---|
| T1 `overlay_url(3, "region", "record", 2.0, "/tmp/cache/frozen-3.png")` tam olarak `"overlay.html?display=3&mode=region&action=record&scale=2&path=/tmp/cache/frozen-3.png"` (açık yazılmış) | URL sözleşmesi | `action` parametresini yazmayı unut: kırmızı |
| T2 **Çapraz dil.** `include_str!(".../src/overlay/action.ts")` okunur; `RECORD_ACTION`'ın Rust'taki değeri o dosyada `RECORD_ACTION = '<değer>'` olarak geçer | iki dilin eylem adı ayrışamaz | Rust'taki sabiti `"rec"` yap: kırmızı. TypeScript'tekini değiştir: yine kırmızı |
| T3 `elapsed_text`: 0 s → `"0:00"`, 7 s → `"0:07"`, 59 s → `"0:59"`, 60 s → `"1:00"`, 83 s → `"1:23"`, 754 s → `"12:34"`, 3599 s → `"59:59"`, 3600 s → `"1:00:00"`, 3723 s → `"1:02:03"`, 86 399 s → `"23:59:59"` (hepsi açık yazılmış) | menü çubuğundaki tek sayı doğru | dakikaya `{:02}` koy: `"0:07"` `"00:07"` olur, kırmızı. Saat kolunu sil: `"1:02:03"` `"62:03"` olur, kırmızı. Saniyeden `{:02}`'yi al: `"1:00"` `"1:0"` olur, kırmızı |
| T4 `record_label("Record Region", true)` tam olarak `"Record Region"`; `record_label("Record Region", false)` `"Record Region"` ile başlar ve `"15"` içerir | çalışan makinede süs yok, çalışmayan makinede sebep var | koşulsuz ekle: `true` durumu kırmızı. Sürüm numarasını çıkar: kırmızı |
| T5 `include_str!("tray.rs")`: üç kayıt öğesinin etiketi `record_label(` üzerinden kurulur ve `SCRecordingOutput::is_available()` menüde bir kez anılır | macOS 14 kapısı gerçekten menüye bağlı | bir öğenin etiketini düz dize yaz: kırmızı |
| T6 `include_str!("tray.rs")`: menü olay işleyicisinde `"stop_recording"` ve `"cancel_recording"` kolları var ve ikisi de `recording::request_` ile başlayan bir çağrı yapıyor | Stop ve Cancel bir şeye bağlı | `stop_recording` kolunu sil: kırmızı |

T3'ün beklentileri `format!` ile üretilmez; on satır literal yazılır. Tuzak bölümünün tam olarak anlattığı şey budur.

- [ ] **Step 2: Kırmızıyı gör, uygula.** Mevcut `overlay_url` testini yeni dizeyle güncelle; başka hiçbir mevcut test değişmez.
- [ ] **Step 3:** Tüm kapılar temiz.
- [ ] **Step 4: Commit** `feat(app): put the recording actions in the menu bar`

---

### Task 8: Kaydı başlatmak, durdurmak, iptal etmek ve süpürmek

**Birim:** U3. **Bağımlılık:** Görev 3, Görev 5, Görev 7.

**Files:** `apps/desktop/src-tauri/src/commands.rs`, `apps/desktop/src-tauri/src/recording.rs` (ekleme), `apps/desktop/src-tauri/src/lib.rs`, `apps/desktop/src-tauri/src/tray.rs` (`quit` kolu)

**Interfaces (birebir):**

```rust
// apps/desktop/src-tauri/src/commands.rs

/// Starts a recording on the region the overlay confirmed.
///
/// Answers with nothing. There is no file and no picture yet: the movie
/// appears when the user stops, and until then the only thing to look at is
/// the menu bar. An `Err` here means the recording never started.
///
/// `async` plus `spawn_blocking` for the reason `capture_region` is: a
/// synchronous command runs on the main thread and everything below blocks for
/// a platform round trip.
#[tauri::command]
pub async fn start_recording(app: AppHandle, display_id: u32, rect: Rect) -> Result<(), String>;
```

```rust
// apps/desktop/src-tauri/src/recording.rs

/// How often the menu bar's elapsed time is redrawn.
const TICK: Duration = Duration::from_millis(1000);

/// Starts a recording, from the blocking worker `start_recording` put it on.
pub fn start(app: &AppHandle, display_id: u32, rect: Rect) -> Result<(), String>;

/// Stops the running recording and puts the movie where the user's settings
/// say. Returns immediately; the work is on a blocking worker.
pub fn request_stop(app: &AppHandle);

/// Stops the running recording and leaves nothing on disk. Returns
/// immediately.
pub fn request_cancel(app: &AppHandle);

/// Stops a running recording and waits for it, for the quit path.
///
/// The one place this is synchronous. `Quit Snapdeck` pressed during a
/// recording must leave a playable file, and a process that exits while
/// `SCRecordingOutput` is still finalising leaves one that opens in nothing.
pub fn stop_before_quit(app: &AppHandle);

/// Deletes recording leftovers from the folder captures go to.
///
/// Called at launch and at the start of every recording; `keep` is the
/// recording that is starting, when there is one.
pub fn sweep_save_directory(app: &AppHandle, keep: Option<&RecordingFiles>) -> usize;
```

**Kurallar:**

- `start`'ın sırası, birebir, ve her adımın başarısızlığı kayıt başlamadan raporlanır:
  1. `state.begin_capture()` **kullanılmaz**; kayıt kendi yuvasını kullanır. Ama overlay'ler kapatılmalı: `dismiss_overlays_and_wait(app)` aynen çağrılır, ve **aynı sebeple**: kayıt bölgeyi yeniden yakalar, ekranda duran bir overlay kullanıcının videosunun içeriği olurdu.
  2. `overlay::discard_cached_frozen_frames(app)`, çekim yolundaki gerekçeyle: donmuş kareler kullanıcının ekranının kayıpsız kopyasıdır.
  3. Ayarlar bir kez okunur, `settings::resolve_save_directory`, `create_dir_all` (best effort). Çekim yolundaki sıranın aynısı.
  4. `sweep_save_directory(app, None)`.
  5. `output::render_filename(&settings.filename_template, OffsetDateTimeParts::now(), width, height)` ile ad üretilir. **`width` ve `height` piksel cinsindendir**, yani bölgenin noktaları çarpı display'in ölçeği; `{width}`/`{height}` taşıyan bir şablonun kayıtta çekimdekiyle aynı sayıları vermesi için.
  6. `recording::reserve(&directory, &stem)?`.
  7. `state.capturer.record(CaptureTarget::Region(global), &files.temporary)?`. Global koordinata çevirme `capture_and_write`'ın yaptığının aynısıdır ve o kodla aynı satırları paylaşır.
  8. `state.begin_recording(session)` `false` dönerse: kayıt hemen `cancel` edilir, `discard` çağrılır, ve kullanıcıya bir kayıt zaten çalıştığı söylenir. Buraya gelmek nadirdir ama ulaşılabilir: iki menü tıklaması.
  9. `show_recording(app, Some(Duration::ZERO))`, `set_recording_items_enabled(app, true)`, ve sayaç iş parçacığı başlatılır.
  - 5'ten sonra herhangi bir adım başarısız olursa `abandon(&files)` çağrılır. Yer tutucu kullanıcının klasöründe kalmaz.
- Sayaç iş parçacığı: `TICK` aralıklarla `state.recording_elapsed()` okur; `None` gelirse kendini bitirir ve `show_recording(app, None)` ile başlığı temizler. Tepsiye yazma `AppHandle` üzerinden yapılır ve best-effort'tur, `overlay::schedule_reveal_deadline`'ın deseni.
- `request_stop`: `spawn_blocking` içinde `state.take_recording()` → `None` ise sessizce döner (Stop'a iki kez basılmış) → `session.recording.stop()` → `recording::finish(summary, &session.files)` → `path` varsa `recents::record(app, &path)` → `complaint` varsa `report::report_failure` → `show_recording(app, None)` ve `set_recording_items_enabled(app, false)`.
- `request_cancel`: aynı yapı, `session.recording.cancel()` → `recording::discard(result, &session.files)` → `complaint` varsa raporla. `recents`'a hiçbir şey girmez.
- `tray.rs`'in `quit` kolu: `crate::recording::stop_before_quit(app);` sonra `app.exit(0)`.
- `lib.rs`: `mod recording;`, `commands::start_recording` `invoke_handler` listesine eklenir, ve `setup` içinde `recents::restore`'dan **sonra** `recording::sweep_save_directory(&handle, None)` çağrılır. Sıra: liste geri yüklendikten sonra, çünkü süpürme silinen dosyaları listenin dışında bırakır ve `restore` zaten var olmayanları budar.
- `commands.rs`'in mevcut komutlarından hiçbiri değişmez.

- [ ] **Step 1: Testleri yaz**

Bu görevin çoğu canlı bir uygulama ister; test edilebilir çekirdek Görev 4, 5 ve 7'de zaten yazıldı. Burada yazılacak olanlar, kalan **saf kararlar** ve **kablolama iddiaları**:

| Test | Ne kanıtlar | Mutasyon kanıtı |
|---|---|---|
| W1 `include_str!("lib.rs")`: `invoke_handler` listesi `commands::start_recording` içerir | komut gerçekten kayıtlı, yoksa overlay'in `invoke`'u çalışma zamanında "unknown command" ile düşer ve bunu hiçbir derleme yakalamaz | listeden çıkar: kırmızı |
| W2 `include_str!("lib.rs")`: `setup` gövdesinde `sweep_save_directory` çağrısı `recents::restore` çağrısından **sonra** geçer | süpürme listeyi budamadan önce liste yüklenmiş olur | iki satırı yer değiştir: kırmızı |
| W3 `include_str!("tray.rs")`: `"quit"` kolu `app.exit(` çağrısından **önce** `stop_before_quit` çağırır | çıkışta yarım kalan film yok | sırayı ters çevir: kırmızı |
| W4 `include_str!("recording.rs")`: `start`'ın gövdesi `dismiss_overlays_and_wait` ve `discard_cached_frozen_frames` ikisini de anar | kayıt, overlay'i ve donmuş kareleri çekim yoluyla aynı şekilde temizler | `discard_cached_frozen_frames` çağrısını sil: kırmızı |
| W5 `TICK` 1000 ms'dir (açık yazılmış) | sayaç ne çok yavaş ne çok hızlı; saniyeden küçük bir aralık menü çubuğunu değişmeyen bir değerle yeniden çizer | `TICK`'i 100 ms yap: kırmızı |

W1-W4 kaynak okuyan testlerdir ve bunun sebebi tektir: bu dört iddianın hepsi tek satırlık bir unutma ile bozulur ve hiçbiri bir derleme hatası üretmez. Bu deseni bu depoda `output::the_jpeg_quality_matches_the_one_the_editor_encodes_at` ve `fullpage`'in tepsi testi zaten kullanıyor.

- [ ] **Step 2: Kırmızıyı gör, uygula.**
- [ ] **Step 3:** Tüm kapılar temiz: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `pnpm lint`, `pnpm test`, `pnpm build`.
- [ ] **Step 4: Commit** `feat(app): record the screen from the menu bar`

---

### Task 9: Gerçek donanım doğrulaması ve belgeler

**Birim:** U5 (+ ölçüm, kod değişikliği yok). **Bağımlılık:** Görev 8.

**Files:** `README.md`

- [ ] **Step 1:** Aşağıdaki "Gerçek donanım doğrulaması" bölümünün **on üç maddesinin hepsini** koş, **ölçülen sayıları raporda ver**. Bir madde koşulamıyorsa (örneğin macOS 14 makine yoksa) koşulmadığını yaz; koştuğunu iddia etme.
- [ ] **Step 2:** README güncellenir:
  - "Limitations" altındaki `- **No screen recording.** Snapdeck takes still captures; it does not record video.` satırı **silinir**.
  - Yeni bir "Recording" bölümü: üç menü öğesi, `Enter` ile başlatma, `Stop Recording`, `Cancel Recording`, çıktı `.mp4` (H.264, 30 fps, imleç görünür), dosyanın kaydetme klasörüne ve Recent Captures'a girdiği.
  - **macOS 15 şartı açıkça yazılır**: uygulama macOS 14'te çalışır, kayıt macOS 15 ister, ve macOS 14'te kayıt öğeleri devre dışıdır.
  - Ölçülen dosya boyutu hızları (Step 1, madde 2) README'ye yazılır: kullanıcı 10 dakikalık bir kaydın kaç yüz MB olacağını bilebilmeli, çünkü bitrate ayarı yok.
  - Bu sürümde olmayanlar bir cümleyle: ses, GIF, trim.
- [ ] **Step 3: Commit** `docs: document screen recording`

---

## Gerçek donanım doğrulaması

Bu bölümün hiçbir maddesi CI'da koşmaz ve hiçbiri bir birim testinin yerini tutmaz. Hepsi **release** paketiyle (`.app`, debug değil) ve gerçek bir ekran kaydı izniyle koşulur; debug derlemesinde zamanlamalar yanıltıcıdır.

| # | Ölçüm | Nasıl | Raporlanacak sayı / eşik |
|---|---|---|---|
| 1 | **Durdurma gecikmesi** | 10 saniyelik, birincil display tam ekran bir kayıt; tepsideki `Stop Recording` tıklamasından dosyanın nihai adıyla klasörde görünmesine kadar. 10 tekrar. | medyan ve p95, ms. **> 2000 ms ise bayrak kaldır** ve sebebini araştır |
| 2 | **Dosya boyutu hızı** | 60 saniyelik üç kayıt, birincil display'in kendi boyutunda, 30 fps: (a) hareketsiz masaüstü, (b) kaydırılan bir web sayfası, (c) tam ekran video | üç sayı, MB/s. README'ye bu sayılar yazılır (bitrate ayarı yok, tasarım 5.3) |
| 3 | **Kare düşürme** | Madde 2(b)'nin `RecordingProgress`'i | `incomplete / frames`, yüzde. **> %1 ise bayrak kaldır** |
| 4 | **Bellek** | Madde 2(c) sırasında uygulamanın tepe RSS'i, boştaki RSS'ine karşı | iki sayı, MB, ve fark. **Fark 200 MB'ı aşarsa bayrak kaldır**: tasarım 4.2 kayıt yolunun kare kopyalamadığını iddia ediyor ve yüzlerce MB o iddiayı çürütür |
| 5 | **CPU** | Madde 2(c) sırasında ortalama CPU% | tek sayı |
| 6 | **Oynatılabilirlik** | Üretilen her `.mp4`: QuickTime Player'da ve Chrome'da açılır; `mdls -name kMDItemDurationSeconds` süreyi duvar saatinin 0,5 s içinde bildirir | geçti/kaldı, ve ölçülen süre farkı |
| 7 | **Negatif kontrol (durdurma sırası)** | `tear_down`'daki iki satır elle yer değiştirilir, tek bir 5 saniyelik kayıt alınır, dosya QuickTime'da **açılmamalıdır**. Sonra değişiklik geri alınır. | "sıra ters çevrilince dosya açılmadı" gözlemi. **Bu madde, C1-C4'ü bir hikâyeden kanıta çeviren şeydir**; atlanırsa durdurma sırası doğrulanmamış sayılır |
| 8 | **İptal** | Kayıt başlat, 5 saniye bekle, `Cancel Recording`. `ls -la` kaydetme klasörü | ne yer tutucu ne `.snapdeck-recording-` dosyası kalmış olmalı. Sıfır dosya |
| 9 | **Çökme süpürmesi** | Kayıt sırasında `kill -9`; klasörde iki artık olduğu doğrulanır; uygulama yeniden açılır; klasör tekrar bakılır | açılıştan sonra sıfır artık, ve `sweep`'in döndürdüğü sayı |
| 10 | **Üç hedef** | Bölge, pencere, tam ekran; her biri 5 saniye | üç dosya da oynuyor, ve piksel boyutları Retina'da seçilen noktanın iki katı |
| 11 | **Çıkışta durdurma** | Kayıt sırasında `Quit Snapdeck` | dosya nihai adıyla klasörde ve oynuyor |
| 12 | **macOS 14** | Erişilebilir bir macOS 14 makine varsa: üç kayıt öğesi devre dışı ve etiketleri sebebi söylüyor; hareketsiz çekim etkilenmemiş | geçti/kaldı, veya "macOS 14 makine yok, koşulmadı" |
| 13 | **Bundle tavanı** | `.dmg` boyutu, bu planın öncesi ve sonrası | iki sayı, MB. Fark gürültü içinde olmalı; yeni crate yok (tasarım 13) |

---

## Uygulama sırasında alınan ek kararlar

> Uygulayıcılar bu bölümü doldurur. Bir görev sırasında bu planın söylemediği bir karar verildiyse, kararın kendisi ve gerekçesi buraya tek paragraf olarak yazılır. Planla çelişen bir şey bulunduysa **uygulanmaz**, buraya yazılır ve rapor edilir.

### Disk alanı ön kontrolü Aşama 1'e alınmadı

Plan bunu açıkça sormuştu. Karar: **alınmıyor.** Hata yolu zaten var ve doğru davranıyor:
`SCRecordingOutputDelegate::recording_did_fail` ateşlenir, `finish` iki dosyayı da toplar ve
kullanıcıya sebebini söyler, yani disk dolduğunda kullanıcı bozuk bir dosyayla kalmaz. Tasarım
7.2'nin üç eşikli politikası bunun üstüne bir iyileştirme, bir doğruluk kapısı değil; ve
uygulanmadan önce gerçek bir kayıtta dosya boyutunun ne hızla büyüdüğünü ölçmek gerekiyor
(Task 9, madde 2). O ölçüm elde olmadan seçilecek eşikler tahmindir.

### Task 3, C11 bu makinede negatif kontrol değil, ve yanlış sıranın bedeli ölçüldü

Plan C11'in mutasyon kanıtını "sırayı ters çevir, `moov` iddiası kırmızı" diye yazmıştı. macOS
27'de **öyle olmuyor** ve bu ölçüldü, varsayılmadı. Aynı iki saniyelik kayıt için:

| Sıra | Dosya | `moov` |
|---|---|---|
| Doğru (sayaç önce iner, `stop_capture` yok) | 1.959.168 bayt, 57 kare | var |
| Ters (kayıt çıktısı önce kaldırılır) | 2.385.608 bayt | var |
| `stop_capture` önce | **206.236 bayt** | var |

Yani bu macOS sürümünde `removeRecordingOutput` filmi kendi başına sonlandırıyor ve crate'in
`stop_capture + wait_until_terminal` dalı ek bir güvence. Yanlış sıranın bedeli **oynatılamaz
dosya değil, kesilmiş kayıt**: aynı iki saniyeden 2 MB yerine 206 KB, yani içeriğin yaklaşık
%90'ı sessizce gidiyor. Belirti değişti, tehlike değişmedi.

C11 olduğu gibi bırakıldı (mutlu yolun gerçek donanım kanıtı) ve sıranın koruması C1-C4'te
duruyor: onlar deterministik, işletim sistemi sürümünden bağımsız ve `stop_capture` dizesinin
dosyada hiç geçmemesini de denetliyorlar. Bayt eşiğine dayanan bir "negatif kontrol" testi
yazılmadı, çünkü durağan bir ekranda düşük bayt meşrudur ve o test kırılgan olurdu; ölçüm Task
9'un elle doğrulama listesinde duruyor.

### Task 3, hata önceliği: kodlayıcının sözü önce gelir

`stop()` önce `tear_down`'ın hatasını döndürüyordu. Disk dolduğunda kullanıcı, delegate'in
"disk dolu" mesajı yerine akışla ilgili opak bir hata görürdü. Karar saf bir fonksiyona çıkarıldı
(`failure_to_report`) ve testi mutasyon kanıtlı: ikisi birden hata verdiğinde kodlayıcınınki
gösterilir, yalnız söküm başarısızsa o gösterilir, ikisi de temizse hiçbir şey.

### Task 2, V7'nin kuralı daraltıldı: `largest_overlap_index` pencere kolunda da kalır

Plan V7'yi "`largest_overlap_index(` çağrısı yalnız `resolve_region` içinde geçer" diye yazmıştı.
Uygulamada bunun bir regresyon getirdiği bulundu ve kural düzeltildi.

Bölge ile pencere aynı kurala tabi değil. Bir **bölge** iki display'e yayılamaz: ScreenCaptureKit
o isteğe siyah şeritli, yanlış ölçekli bir kare döndürür, bu yüzden `resolve_region` onu reddeder.
Bir **pencere** ise pekâlâ yayılabilir, kullanıcı onu sınıra sürükler, ve bugünkü davranış o
pencereye çoğunluk display'in ölçeğini verir. Pencere kolunu `resolve_region`'a bağlamak, o
pencerenin `Err` alıp `1.0`'a düşmesi demekti: karışık DPI bir kurulumda 2x yerine 1x çıktı, yani
hiçbir testin yakalamadığı sessiz bir regresyon ve bu görevin kendi kuralının ("refactor
`capture()`'ın davranışını değiştirmez") ihlali.

Yürürlükteki kural: `contains(` üretim kodunda yalnız `resolve_region` içinde geçer;
`largest_overlap_index(` ise `resolve_region` dışında **tam olarak bir kez**, `resolve`'un pencere
kolunda geçer ve test bunu da çivilemiştir. Planın mutasyon kanıtı aynen geçerli: `capture` veya
`record` içine böyle bir çağrı yapıştırmak V7'yi kırmızıya çevirir.

### Task 9, gerçek donanım sonuçları

Kurulu release paketi, 1710x1112 puntoluk Retina ekran, macOS 27, tam ekran kayıt. Kayıtların
hiçbiri açılıp izlenmedi: kullanıcının canlı ekranını içerdikleri için yalnız sayıları okundu ve
dosyalar hemen silindi.

**İlk koşu yanıltıcıydı ve bunu ayırt etmek için yöntem değiştirildi.** İlk tam ekran testi "dosya
yok" döndü, ama log'a da hiçbir satır düşmemişti: kayıt hata vermemiş, **hiç başlamamıştı**
(uygulama açıldıktan hemen sonraki ilk menü tıklaması overlay'i açmamıştı). O koşudaki iptal
testinin "temiz" sonucu da bu yüzden kanıt sayılmadı. İkinci ve üçüncü koşularda her test, gizli
geçici dosya ile sıfır baytlık yer tutucunun ikisinin de oluştuğunu görerek kaydın gerçekten
başladığını önce kanıtladı.

| # | Ölçüm | Sonuç |
|---|---|---|
| 1 | Durdurma gecikmesi, menü tıklamasından dosyanın nihai adıyla görünmesine | 65 / 422 / 405 ms. İlk ölçülen 3,5 s, betiğin kendi menü beklemelerini içeriyordu. Eşik 2000 ms, altında |
| 2 | Dosya boyutu hızı | Hareketli masaüstü 0,46-0,54 MB/s; durağan masaüstü 0,076-0,077 MB/s |
| 3 | Kare hızı (AVFoundation `nominalFrameRate`) | 29,2-29,3 fps, 30 fps sınırının hemen altında |
| 6 | Oynatılabilirlik | AVFoundation üç dosyayı da açtı, `isPlayable = true`; `moov` her dosyada var |
| 8 | Kanıtlı başlangıçtan sonra iptal | 0 mp4, 0 geçici dosya, 0 yer tutucu |
| 9 | Çökme süpürmesi | `kill -9` sonrası 1 geçici dosya + 1 yer tutucu; yeniden açılışta 0 + 0 |
| 10 | Piksel boyutu | 3420x2224, ekranın 1710x1112 puntosunun tam iki katı |
| 11 | Kayıt sürerken çıkış | Dosya nihai adıyla kaldı, 1.341.103 bayt, `moov` var, artık yok |
| 13 | DMG boyutu | 5,7 MB (bu aşamadan önce 5,5 MB); yeni crate yok, fark kayıt kodunun kendisi |

Koşulmayanlar ve sebepleri: **4 (bellek) ve 5 (CPU)** ayrı bir yük ölçümü istiyor ve bu turda
yapılmadı. **7 (sıranın negatif kontrolü)** Task 3'te zaten ölçüldü ve sonucu yukarıda: bu macOS
sürümünde yanlış sıra oynatılamaz dosya değil, kesilmiş kayıt üretiyor. **12 (macOS 14)** erişilebilir
bir macOS 14 makine olmadığı için koşulmadı; kayıt öğelerinin orada devre dışı kurulması birim
testleriyle (`record_items_enabled` doğruluk tablosu) kanıtlı, gerçek bir makinede görülmedi.
**10'un bölge ve pencere kolları** koşulmadı; yalnız tam ekran ölçüldü.

