# Snapdeck Ekran Kaydı, Tasarım Dokümanı

- Tarih: 2026-09-14
- Durum: onaylandı (2026-09-14), bölüm 16'daki kararlarla
- Kapsam: v2 ekran kaydı (MP4, sistem sesi ve mikrofon, GIF, trim)
- Önkoşul: `docs/superpowers/specs/2026-09-07-snapdeck-design.md` (v1, onaylandı)
- Taban: macOS 14.0+ uygulama, **kayıt için macOS 15.0+** (bölüm 4.3)

## 1. Amaç

Snapdeck bugün hareketsiz kare çekiyor. Bu tasarım, aynı menü çubuğu uygulamasına ekran kaydını
ekler: bölge, pencere veya tam ekran, MP4 dosyası, ses, ve baş/son kırpma. Kaydedilen dosya
ikinci bir kaydetme yolu açmaz; kullanıcının klasörü, ad şablonu ve Recent Captures listesi neyse
kayıt da oraya girer.

Ürünün iki iddiası bu tasarımın sınırını çiziyor ve ikisi de pazarlama değil, ölçülebilir taahhüt:
uygulama hafif (DMG bugün 5,5 MB) ve istenmedikçe ağa çıkmıyor. Bir video kodlayıcı bu iki
iddianın da en büyük tehdidi olduğu için, bölüm 5'teki karar tek başına bu dokümanın ağırlık
merkezidir.

## 2. Kapsam sınırı

### Kapsam içi
- Bölge, pencere ve tam ekran kaydı; MP4 (H.264).
- Kayıt sırasında durdurma ve iptal, geçen süre göstergesi.
- Sistem sesi ve mikrofon, ayrı ayrı açılıp kapanabilir.
- GIF çıktısı ve trim (baş/son kırpma).
- Dosyanın mevcut kaydetme akışına girmesi: klasör, ad şablonu, çakışma soneki, Recent Captures.

### Kapsam dışı (bu dokümanın konusu değil)
- İmleç vurgusu, tıklama efekti, klavye tuşu göstergesi. v1 dokümanında ayrı bir v2 maddesi;
  burada yalnızca imlecin videoda **görünmesi** var, vurgulanması yok.
- Video üzerinde annotation, altyazı, zoom, hız değiştirme, kesme (baş/son dışı).
- Webcam, resim içinde resim.
- Panoya kopyalama. Bir video panoya konmaz; gerekçe bölüm 9.4.
- Windows ve Linux kaydı. Trait bölüm 6'da hazır, implementasyon yok.
- Kayıt için ayrı global kısayol. Menü çubuğu yeterli; kısayol ayrı bir istektir.
- `packages/editor` içinde hiçbir değişiklik. Video düzenleme editörün işi değil (bölüm 10.4).

## 3. Aşamalar

Her aşama tek başına yayınlanabilir bir üründür ve kendinden öncekini bozmaz.

### 3.1 Aşama 1: sessiz MP4

**Ne yapılacak.** Menü çubuğuna üç kayıt eylemi (bölge, pencere, tam ekran). Mevcut seçim
overlay'i aynen kullanılır; `Enter` çekim yerine kaydı başlatır. Kayıt sürerken menü çubuğu
geçen süreyi gösterir ve menüde `Stop Recording` ile `Cancel Recording` bulunur. Durdurulunca
dosya, ayarlardaki klasöre, ayarlardaki ad şablonuyla, `.mp4` uzantısıyla yazılır ve Recent
Captures'a girer.

**Ne yapılmayacak.** Ses yok, GIF yok, trim yok, ayar penceresinde yeni alan yok, editör
açılmıyor, panoya kopyalanmıyor.

**Neden önce bu.** Boru hattının tamamı (SCStream, `SCRecordingOutput`, dosya sonlandırma,
geçici dosya ve rename, disk kontrolü, tepsi durumu, çökme davranışı) bu aşamada kurulur. Ses ve
GIF, bu iskelet oturmadan test edilebilir değil. Aşama 1 tek başına da "artık ekran kaydı var"
demeye yeter; README'nin "No screen recording" satırı burada düşer.

**Bitti kriteri.** Üç hedefte de kayıt başlıyor, durduruluyor, iptal ediliyor; çıkan dosya
QuickTime'da açılıyor; ad şablonu, çakışma soneki ve Recent Captures hareketsiz çekimdekiyle
aynı davranıyor; iptal edilen kayıt diskte hiçbir şey bırakmıyor.

### 3.2 Aşama 2: ses

İki adımda, çünkü ikisinin riski aynı değil (bölüm 5.4).

**2a, sistem sesi.** `SCStreamConfiguration.capturesAudio` açılır,
`excludesCurrentProcessAudio` da açılır (Snapdeck'in kendi sesi kaydedilmez, geri besleme
döngüsü olmaz). Yeni izin gerekmez: sistem sesi ekran kaydı izninin altındadır (bölüm 8.2).
Ayar penceresine tek bir onay kutusu gelir.

**2b, mikrofon.** `SCStreamConfiguration.captureMicrophone` (macOS 15.0+) açılır. Ayrı bir TCC
izni ve `Info.plist` içinde `NSMicrophoneUsageDescription` gerektirir. **Bu adımın ilk işi bir
ölçüm görevidir**, uygulama değil: Apple'ın forumunda `captureMicrophone = true` iken
`SCRecordingOutput`'un bozuk MP4 ürettiği bildirilmiş ve kapanmamış (bölüm 15, kaynak 6). Ölçüm
hedef macOS sürümlerinde bunu doğrularsa 2b bu haliyle yayınlanmaz; bölüm 5.4'teki geri çekilme
yolu uygulanır veya mikrofon bir sonraki sürüme bırakılır.

**Ne yapılmayacak.** Ses seviyesi göstergesi, giriş cihazı seçimi, gürültü azaltma, sesin ayrı
dosyaya yazılması, sistem sesi ile mikrofonun ayrı seviyelerde miksajı.

**Neden bu sırada.** Ses, aşama 1'in dosya yolunu hiç değiştirmez: aynı `SCStream`'e iki
yapılandırma bayrağı eklenir. Buna karşılık mikrofon, bu tasarımın bilinen tek dış hatasını
taşıyor; aşama 1 yayındayken ölçmek, aşama 1'i geciktirmekten iyidir.

### 3.3 Aşama 3: GIF ve trim

**Ne yapılacak.** Kayıt her zaman önce MP4 olarak yazılır. Bitince, ayar açıksa küçük bir trim
penceresi açılır: klip, baş ve son tutamağı, `Save` ve `Close`. `Save`, seçilen aralığı
`AVAssetExportSession` ile passthrough olarak dışa aktarır ve dosyayı rename ile değiştirir,
tıpkı editörün düzenlenmiş bir çekimi değiştirdiği gibi. Ayarlara `Recording format: MP4 / GIF`
gelir; GIF seçiliyse MP4 geçici dosyadan GIF üretilir ve kullanıcının klasörüne `.gif` yazılır.

**Ne yapılmayacak.** Zaman çizelgesinde ortadan kesme, birden çok aralık, kare kare gezinme,
GIF için palet veya dither seçimi, sonsuz döngü dışında bir tekrar ayarı.

**Neden en sonda.** İkisi de bitmiş bir MP4 üzerinde çalışan **dışa aktarma** işleridir; kayıt
boru hattına hiç dokunmazlar. Ayrıca ikisi de AVFoundation bağlamalarını tek seferde tree'ye
sokar: ayrı aşamalara bölmek aynı bağımlılığı iki kez gerekçelendirmek olurdu.

## 4. Yakalama boru hattı

### 4.1 `screencapturekit` 9.0.1 kare akışını destekliyor mu

Evet, iki ayrı yoldan. Depodaki sürümün kaynağı okundu
(`~/.cargo/registry/src/index.crates.io-*/screencapturekit-9.0.1/`):

| Yol | API | Özellik bayrağı | macOS |
|---|---|---|---|
| Kare kare teslim | `SCStream::add_output_handler(handler, SCStreamOutputType::Screen)`, `CMSampleBuffer` verir | yok, tabanda var | 13.0+ |
| Doğrudan dosyaya kayıt | `SCRecordingOutput` + `SCStream::add_recording_output` | `macos_15_0` | 15.0+ |
| Sistem sesi | `SCStreamConfiguration::with_captures_audio(true)` | yok, tabanda var | 13.0+ |
| Mikrofon | `SCStreamConfiguration::with_captures_microphone(true)` | `macos_15_0` | 15.0+ |

`CMSampleBuffer` üzerinden `frame_status()`, `presentation_timestamp()` ve `image_buffer()`
okunabiliyor; `image_buffer()` bir `IOSurface` veriyor, yani kareler GPU belleğinde ve kopyasız.

### 4.2 v1 dokümanından sapma: `stream()` eklenmiyor

v1 dokümanı şunu öngörmüştü: *"v2'de `ScreenCapturer` trait'ine `stream()` metodu eklenir;
`Frame` tipi stride, pixel format ve timestamp taşıdığı için bu ekleme mevcut çağıranları
kırmaz."* İkinci yarısı doğruydu, birincisi yanlış çıktı. Gerekçe aritmetik:

`Frame.data` sahipli bir `Vec<u8>`'dir. Kareyi `Frame`'e çevirmek, `IOSurface`'ten yığına bir
memcpy demektir:

| Ekran | Kare başına | 30 fps | 60 fps |
|---|---|---|---|
| 3840x2160 (4K) | 33.177.600 bayt (31,6 MiB) | 0,99 GB/s | **1,99 GB/s** |
| 3456x2234 (16" MacBook Pro, native) | 30.882.816 bayt (29,5 MiB) | 0,93 GB/s | 1,85 GB/s |
| 5120x2880 (5K) | 58.982.400 bayt (56,3 MiB) | 1,77 GB/s | **3,54 GB/s** |

Saniyede 2 GB memcpy ve 60 ayrı 32 MB ayırma, kodlayıcıya daha başlamadan verilmiş bir bütçedir.
Üstelik gereksizdir: hem `SCRecordingOutput` hem `AVAssetWriter` `IOSurface`'i doğrudan alır, o
kopya hiç yapılmaz.

Bu yüzden trait'e `stream()` değil `record()` eklenir (bölüm 6). v1'in asıl vaadi, "mevcut
çağıranlar kırılmaz", korunur ve daha güçlü bir mekanizmayla korunur: **varsayılan gövdeli trait
metodu**. `mock.rs` dahil hiçbir mevcut implementasyon tek satır değişmez.

GIF de `stream()` istemiyor: GIF, bitmiş MP4 üzerinden dışa aktarılır (bölüm 5.5).

### 4.3 Kayıt için taban neden macOS 15.0

`SCRecordingOutput` macOS 15.0 API'sidir. Uygulamanın kendi tabanı 14.0'da kalır, ekran görüntüsü
her şeyiyle çalışmaya devam eder; **yalnız kayıt** 15.0 ister ve macOS 14'te menüdeki kayıt
öğeleri devre dışı görünür, tıklanınca ne olduğunu söyleyen bir satır gösterir. Kontrol derleme
zamanında değil çalışma zamanında yapılır: `SCRecordingOutput::is_available()`.

Bugün, 2026-09-14, macOS 27 (Golden Gate) yayınlandı (kaynak 8). Sürüm merdiveni: 14 Sonoma
(2023), 15 Sequoia (2024), 26 Tahoe (2025), 27 (2026). Kaydı 15.0'a bağlamak yalnızca üç sürüm
geride kalmış tek bir macOS'u dışarıda bırakıyor. Bunun karşılığında elde edilen şey bölüm
5'tedir ve küçük değildir: yazılmayan bir video kodlayıcı.

## 5. Kodlama kararı

### 5.1 Seçenekler ve ölçülen bedelleri

Sürümler crates.io API'sinden ve `Cargo.lock`'tan okundu, ezberden yazılmadı.

| Yol | Crate / API | Sürüm, son yayın | Lisans | Bundle etkisi | Yazılacak kod | Taban |
|---|---|---|---|---|---|---|
| **A. `SCRecordingOutput`** | `screencapturekit` (tree'de zaten var) | 9.0.1, 2026-08-31 | MIT OR Apache-2.0 | **0 MB** (sistem framework'ü) | ~150 satır | macOS 15.0 |
| B. AVAssetWriter | `objc2-av-foundation` | 0.3.2, 2025-10-04 | Zlib OR Apache-2.0 OR MIT | ~0 MB (sistem framework'ü) | 500-800 satır, gerçek zamanlı kare pompası | macOS 14.0 |
| C. ffmpeg ikilisi | `ffmpeg-sidecar` | 2.5.2, 2026-05-30 | MIT (sarmalayıcı) | **+40-80 MB** ikili | ~200 satır | macOS 14.0 |
| D. Saf Rust H.264 | yok | crates.io'da bakımlı, üretime uygun bir H.264 encoder yok | - | - | - | - |

C'nin lisansı sarmalayıcının lisansıdır, çalıştırdığı ikilinin değil: x264 ile derlenmiş bir
ffmpeg GPL'dir ve MIT bir ürünle dağıtılamaz; LGPL bir derleme ise H.264 kodlamaz. Bundle etkisi
zaten tek başına elemeye yetiyor: 5,5 MB'lık bir DMG'ye 40-80 MB eklemek, ürünün konumlandığı
cümleyi ortadan kaldırır.

D için crates.io'da ölçülebilir bir aday bulunamadı. Bu bir tahmin değil, arama sonucudur: saf
Rust'ta bakımlı bir donanım hızlandırmalı H.264 kodlayıcı yok.

### 5.2 Karar: A, `SCRecordingOutput`

`SCStream`'e bir `SCRecordingOutput` takılır; H.264, MP4, donanım hızlandırmalı, dosyayı Apple
yazar. Yeni crate yok, yeni ikili yok, bundle'a bayt eklenmiyor. `crates/capture`'ın
`screencapturekit` özelliği `macos_14_0`'dan `macos_15_0`'a yükselir; bu bir sürüm değişikliği
değil, aynı crate'in zaten derlenen bir özelliğidir ve özellikler kümülatiftir.

B, macOS 14'ü de kapsadığı için cazip görünüyor ve bölüm 3.3'te AVFoundation zaten tree'ye
giriyor. Yine de seçilmedi, ve ikisi aynı şey değil: aşama 3'ün kullandığı
`AVAssetExportSession` çağrısı kurulur, `timeRange` verilir ve beklenir; `AVAssetWriter` ise
gerçek zamanlı bir kare pompasıdır, `expectsMediaDataInRealTime`, pts yeniden tabanlama, ses için
ikinci bir input, ve kodlayıcı yetişemediğinde geri basınç. v1'in kendi ifadesiyle kayıt "en ağır
alt sistem"; bu yolun tamamını yazmamak, bu dokümanın verdiği en değerli karardır.

### 5.3 Kodlayıcıya ne söyleniyor

`SCRecordingOutputConfiguration` üç şey kabul ediyor: çıktı URL'i, video codec ve dosya tipi.
Bitrate, keyframe aralığı ve profil ayarı **yok**.

- Codec: `SCRecordingOutputCodec::H264` (`avc1`). HEVC daha küçük dosya verir ama paylaşımda
  H.264 her yerde açılır; ekran kaydının büyük kısmı paylaşılmak için alınır.
- Dosya tipi: `SCRecordingOutputFileType::MP4` (`public.mpeg-4`).
- Kare hızı: `SCStreamConfiguration::with_minimum_frame_interval(CMTime { value: 1, timescale: fps })`,
  varsayılan 30.
- Çözünürlük: hedefin kendi piksel boyutu, `capture()`'ın kullandığı `scale_factor` ile, yani
  Retina'da seçilen nokta sayısının iki katı piksel.
- İmleç: `with_shows_cursor(true)`. Hareketsiz çekimde `false`, kayıtta `true`: bir ekran
  kaydında imleç içeriğin parçasıdır.

Bitrate kontrolü olmaması bölüm 7.2'de disk politikasını etkiliyor: sabit bir sayı varsayamayız,
gerçek hızı kayıt sürerken ölçeriz.

### 5.4 Ses: `SCRecordingOutput` ses de yazar mı

WWDC24'ün kendi ifadesiyle `SCRecordingOutput` "ekran, ses ve mikrofon içeriğini kaydetmenin
basit ve elverişli yolu"dur (kaynak 5). Sistem sesi için bu yol açık ve 2a bu şekilde
uygulanacak.

Mikrofon için değil. Apple Developer Forums 805892 (Kasım 2025, son yanıt Mart 2026, çözülmedi):
`captureMicrophone = true` iken `SCRecordingOutput`'un ürettiği MP4 oynatılamıyor;
`capturesAudio` açık ya da kapalı olması durumu değiştirmiyor. Bildirilen kök neden, sistem sesi
ile mikrofonun farklı `CMFormatDescription` ile geldiği ve tek bir konteynere yazılınca container'ı
bozduğu. Üçüncü taraf bir başka kaynak (kaynak 7), tek geçişte miksajın `mixesAudioWithMicrophone`
istediğini ve bunun yeni bir macOS'ta geldiğini söylüyor; **bu doğrulanamadı**, `screencapturekit`
9.0.1 böyle bir özellik açmıyor ve bu tasarım ona dayanmıyor.

Geri çekilme yolu, 2b'nin ölçümü kötü çıkarsa, öncelik sırasıyla:
1. Mikrofon açıkken `SCRecordingOutput` yerine mikrofonu ayrı bir AAC dosyasına yazmak ve bitince
   `AVMutableComposition` + `AVAssetExportSession` ile MP4'e muxlamak. AVFoundation aşama 3'te
   zaten tree'de; maliyet mikrofon tarafının ayrı yazılması.
2. Mikrofonu bu sürüme almamak ve ayarda göstermemek. Olmayan bir özelliği bozuk sunmaktan iyidir.

Karar ölçüme bağlıdır ve ölçüm 2b'nin ilk görevidir. Yayınlanmış bir Snapdeck'in bozuk MP4
üretmesi kabul edilemez.

### 5.5 GIF

GIF, bitmiş MP4'ten dışa aktarılır. Canlı GIF kodlaması denenmedi çünkü aritmetiği tutmuyor:
`image`'in `GifEncoder`'ı her kare için `color_quant` ile paletleme yapar ve 800x500 bir karede
bu on milisaniyeler sürer; 15 fps'te gerçek zamanlı değildir, kareleri bir yere tamponlamak
gerekirdi, ki o yer diskse zaten MP4'ten daha kötü bir dosya olur.

- Çözme: `AVAssetImageGenerator`, toplu `generateCGImagesAsynchronously(forTimes:)`,
  `requestedTimeToleranceBefore/After = .zero`. Bağlama yüzeyi `AVAssetReader`'dan belirgin
  biçimde küçük. Çok yavaş çıkarsa alternatifi `AVAssetReader` + `AVAssetReaderTrackOutput`'tur;
  kararı verecek ölçüm: 10 saniyelik bir klibi GIF'e çevirme süresi.
- Ölçekleme: `image::imageops::resize`, en fazla `GIF_MAX_WIDTH` piksel genişlik.
- Kodlama: `image` crate'inin `gif` özelliği. Crate zaten bağımlılık (`0.25.10`, `Cargo.lock`),
  yalnız özellik açılır. Tree'ye giren yeni crate'ler `gif` (image 0.25 bunu 0.13.1'e sabitliyor;
  crate'in kendi güncel sürümü 0.14.2, 2026-04-09; lisans MIT OR Apache-2.0) ve `color_quant`
  (image 0.25 bunu 1.1'e sabitliyor; güncel 2.0.0, 2026-05-09; lisans MIT). İkisi de MIT ürünle
  uyumlu.
- `gifski` **elendi**: lisansı AGPL-3.0-or-later (1.34.0, 2025-07-13). Daha iyi palet üretiyor,
  ama MIT bir üründe dağıtılamaz.

GIF kare zamanlaması saf aritmetiktir ve bölüm 12'de test edilir: GIF gecikmesi santisaniye
cinsindendir, 15 fps 6,67 santisaniyedir, bu yüzden gecikmeler 7,7,6,7,7,6 gibi dağıtılır ve
toplamları klibin gerçek süresine eşitlenir. Naif bir `round(6.67) = 7`, 30 saniyelik bir GIF'i
1 saniyeden fazla uzatır.

### 5.6 Trim

`AVAssetExportSession` + `AVAssetExportPresetPassthrough` + `timeRange`. Yeniden kodlama yok,
kalite kaybı yok, birkaç yüz milisaniye.

Bilinen sınır, ve dürüstçe belgelenecek: passthrough kesimi başlangıçta keyframe sınırına
oturur. `SCRecordingOutput` keyframe aralığını ayarlatmadığı için bu aralık bilinmiyor. Aşama
3'ün ölçüm görevi bunu ölçer ve sayıyı README'ye yazar. Ölçülen aralık 2 saniyeden büyükse trim
için yeniden kodlama seçeneği tartışılır; bu, bölüm 14'teki açık kararlardan biridir.

`exportAsynchronously` macOS 15'te deprecated oldu, sınıfın kendisi değil. Yeni `export(to:as:)`
async API'si kullanılır, mevcut değilse eskisine düşülür.

## 6. Trait tasarımı

### 6.1 Değişiklik

```rust
// crates/capture/src/lib.rs
pub trait ScreenCapturer {
    fn displays(&self) -> Result<Vec<DisplayInfo>, CaptureError>;
    fn windows(&self) -> Result<Vec<WindowInfo>, CaptureError>;
    fn capture(&self, target: CaptureTarget) -> Result<Frame, CaptureError>;

    /// Hedefi `destination` yoluna kaydetmeye başlar.
    ///
    /// Varsayılan gövde, kaydı desteklemeyen her implementasyon için
    /// doğru cevabı verir: sessizce boş dosya değil, adı konmuş bir hata.
    /// Bu sayede `mock::MockCapturer` ve ileride eklenecek platformlar
    /// tek satır değişmeden derlenir.
    fn record(
        &self,
        target: CaptureTarget,
        options: &RecordingOptions,
        destination: &Path,
    ) -> Result<Box<dyn RecordingSession>, CaptureError> {
        let _ = (target, options, destination);
        Err(CaptureError::Unsupported(
            "this capturer cannot record".to_string(),
        ))
    }
}

/// Devam eden bir kayıt. Durdurulduğunda veya iptal edildiğinde tükenir,
/// çünkü sonlanmış bir oturum ikinci kez durdurulamaz ve bunu tip sistemi
/// söylemeli, çalışma zamanı değil.
pub trait RecordingSession: Send {
    fn progress(&self) -> RecordingProgress;
    fn stop(self: Box<Self>) -> Result<RecordingSummary, CaptureError>;
    fn cancel(self: Box<Self>) -> Result<(), CaptureError>;
}
```

Mevcut çağıranların kırılmaması, `Frame`'in timestamp taşımasından değil, **varsayılan
gövdeden** gelir. `mock.rs` ve `macos/mod.rs`'teki `impl ScreenCapturer` blokları olduğu gibi
derlenir; yalnız `MacCapturer` `record()`'u ezer.

### 6.2 Yeni düz veri tipleri

`crates/frame` içine, `Frame` ve `Rect` hangi gerekçeyle oradaysa aynı gerekçeyle: piksel ve
geometri dışında hiçbir şey bilmeyen, platform framework'üne bağlanmayan düz veri.

```rust
// crates/frame/src/types.rs
pub struct RecordingOptions {
    /// Saniyedeki kare üst sınırı. SCStreamConfiguration'a
    /// CMTime { value: 1, timescale: fps } olarak geçer.
    pub fps: u32,
    /// Kayıtta true. Hareketsiz çekimde false olması bir tercih değil,
    /// farklı bir sorunun cevabı: bir ekran kaydında imleç içeriktir.
    pub shows_cursor: bool,
    pub audio: AudioSources,       // aşama 2
}

pub struct AudioSources {
    pub system: bool,
    pub microphone: bool,
}

/// Kayıt sürerken okunabilen ilerleme. `written`, duvar saati değil,
/// SCRecordingOutput'un dosyaya gerçekten yazdığı süredir.
pub struct RecordingProgress {
    pub written: Duration,
    pub bytes: u64,
}

pub struct RecordingSummary {
    pub path: PathBuf,
    pub duration: Duration,
    pub bytes: u64,
    pub width: u32,
    pub height: u32,
    /// Akış boyunca teslim edilen kare sayısı; bölüm 7.3.
    pub frames: u64,
}
```

`CaptureError` bir varyant kazanır:

```rust
/// Bu platform ya da bu macOS sürümü istenen işi yapamıyor.
#[error("unsupported: {0}")]
Unsupported(String),
```

Yeni varyant serde ile serileşen bir enum'a eklendiğinden, overlay ve ayar pencerelerindeki
hata ayrıştırması `kind: "unsupported"` değerini tanımalı; tanımayan taraf zaten genel hata
metnine düşüyor, bu yüzden kırılma değil, eksik metin riski var ve o da kapatılacak.

### 6.3 macOS implementasyonu ve zorunlu refactor

`macos/mod.rs`'teki `capture()` bugün üç kolda aynı işi üç kez yapıyor: hedefi çözümle, doğru
display'i bul, `scale_factor`'ü al, `SCContentFilter` kur, piksel boyutunu hesapla. Bölge kolu
ayrıca iki kuralı taşıyor: en çok örtüşen display kazanır (`largest_overlap_index`) ve bölge tek
bir display'in içinde kalmalı (`contains`, aksi halde ScreenCaptureKit siyah şeritli, yanlış
ölçekli bir kare döndürüyor).

Kayıt bu kuralların **hepsine** ihtiyaç duyuyor. İkinci bir kopya çıkarmak, bu iki kuralın
çekimde geçerli olup kayıtta sessizce kaybolması demektir. Bu yüzden çözümleme tek bir özel
fonksiyona çıkarılır:

```rust
struct ResolvedTarget {
    filter: SCContentFilter,
    scale: f32,
    width: u32,
    height: u32,
    source_rect: Option<CGRect>,
}

fn resolve(target: CaptureTarget) -> Result<ResolvedTarget, CaptureError>;
```

`capture()` ve `record()` ikisi de bunu çağırır. Bu bir iyileştirme değil, **istenen işin
çalışması için zorunlu** olan tek refactor'dür ve kapsamı burasıyla sınırlıdır: `resolve` dışarı
açılmaz, `SCContentFilter` crate sınırını geçmez.

### 6.4 Durdurma sırası, kırılgan nokta

`screencapturekit` 9.0.1'in `SCStream::remove_recording_output` gövdesi okundu. Dosyanın
sonlanmasını (`sc_recording_output_wait_until_terminal`) **yalnızca** şu koşulda bekliyor:

```rust
if context.capturing.load(..) && !context.has_handlers() && context.recording_outputs.load(..) == 1
```

`has_handlers()`, stream'e takılı **herhangi bir** output handler varsa true'dur. Bölüm 7.3'teki
kare sayacı bir output handler'dır. Yani sayaç takılıyken `remove_recording_output` çağrılırsa
crate sonlandırmayı beklemez ve MP4 moov atom'u yazılmadan kesilebilir; dosya oynatılamaz.

Doğru sıra, tek bir fonksiyonda toplanır ve bölüm 12'de bir test bu sırayı çiviler:

```
1. stream.try_remove_output_handler(counter_id, SCStreamOutputType::Screen)
2. stream.remove_recording_output(&output)      // burada crate durdurur ve sonlanmayı bekler
3. stream.stop_capture()                        // idempotent, zaten durmuş
```

`cancel()` aynı sırayı izler, sonra geçici dosyayı siler.

## 7. Bellek, disk ve kare düşürme

### 7.1 Bellek

`SCRecordingOutput` yolunda kareler hiçbir zaman Rust yığınına kopyalanmaz; `IOSurface`'ten
kodlayıcıya gider. Snapdeck'in kayıt sırasındaki ek bellek kullanımı, `queue_depth` kadar
`IOSurface` (varsayılan bırakılır) ve birkaç kilobayt oturum durumundan ibarettir. Bölüm 4.2'deki
1,99 GB/s tablosu, seçilmeyen yolun bedelidir ve bu yüzden orada duruyor.

### 7.2 Disk

Bitrate ayarlanamadığı için (bölüm 5.3) sabit bir "saatte şu kadar GB" sayısı varsayılmaz.
Aritmetik şu: 1 Mbit/s = 450 MB/saat. Ekran içeriği için tipik donanım H.264 aralığı geniştir,
bu yüzden politika **ölçülen** hız üzerine kurulur.

Kayıt sürerken saniyede bir, `SCRecordingOutput::recorded_file_size()` ve geçen süre ile gerçek
hız hesaplanır, `statfs` ile boş alan okunur (`libc` 0.2.189 zaten `Cargo.lock`'ta, yeni crate
değil), ve saf bir fonksiyon karar verir:

```rust
enum DiskVerdict { Fine, Warn { seconds_left: u64 }, StopNow }

fn disk_verdict(free_bytes: u64, written_bytes: u64, elapsed: Duration) -> DiskVerdict;
```

Eşikler, isimlendirilmiş sabitler ve gerekçeleri:

| Sabit | Değer | Neden |
|---|---|---|
| `START_FLOOR_BYTES` | 1 GiB | Yarım yazılmış bir MP4 oynatılamaz, kısmen kurtarılamaz. Başlamamak, sonra kesmekten iyidir. |
| `RESERVE_BYTES` | 256 MiB | Sonlandırma sırasında macOS'un kendisinin yer bulabilmesi için. Hesaplanan boş alandan düşülür. |
| `WARN_SECONDS_LEFT` | 60 | Kullanıcının kaydı bitirmeye karar verebileceği en kısa makul süre. Tepsi başlığında uyarı. |
| `STOP_SECONDS_LEFT` | 15 | Bu noktada kayıt kendiliğinden düzgünce durdurulur ve dosya sonlandırılır. |

Kullanıcıya "yer kalmadı" üç anda söylenir: başlamadan önce (kayıt hiç başlamaz),
`WARN_SECONDS_LEFT` altına inince (tepsi uyarır, kayıt sürer), ve `STOP_SECONDS_LEFT` altına
inince (kayıt durur, dosya geçerlidir, kullanıcıya neden durduğu söylenir). Hiçbirinde sessizce
bozuk dosya bırakılmaz.

### 7.3 Kare düşürme

Kodlayıcı yetişemediğinde `SCRecordingOutput` kare düşürür. Sonuç, videonun **kısalması değil**,
akıcılığının düşmesidir: pts'ler gerçek zamandır, MP4 değişken kare hızlıdır ve duvar saatiyle
doğru hızda oynar. Bu davranış Apple'ındır ve değiştirilemez; bizim seçebildiğimiz tek şey üst
sınırdır (`minimum_frame_interval`), ve 60 yerine 30'u varsayılan yapmak düşme olasılığını
belirgin biçimde azaltır.

Sorun şu: `SCRecordingOutput` kare sayacı vermiyor. `recorded_duration()` pts aralığını verir ve
kareler düşse de duvar saatini takip eder, yani düşmeyi göremez. O yüzden uygulama, düşme olduğunu
**bilemez** ve bilmediğini iddia edemez.

Bu yüzden aşama 1'e, aynı stream'e takılan bir sayaç handler'ı eklenir: `CMSampleBuffer`'ı alır,
yalnız `frame_status()` okur, pikseline dokunmaz ve düşürür. Maliyeti ölçülebilir biçimde sıfıra
yakındır (kopya yok) ve karşılığında iki şey verir: `RecordingSummary.frames` ve "kayıt
yetişemiyor" sinyali (gerçekleşen fps, istenen fps'in belirgin altındaysa). Bedeli bölüm 6.4'teki
durdurma sırası kuralıdır ve o kural bir testle çivilenir.

Sayaç sorun çıkarırsa çıkarılabilir; o zaman `frames` alanı düşer ve README "Snapdeck kaç kare
düştüğünü söyleyemez" der. Sessizce yanlış bir sayı vermez.

## 8. İzinler

v1 kuralı aynen sürer: izin açılışta değil, kullanım anında istenir.

### 8.1 Ekran kaydı

Yeni izin yok. Kayıt, hareketsiz çekimin kullandığı TCC iznini kullanır.
`CGPreflightScreenCaptureAccess` her kayıttan önce, `capture()` öncesinde olduğu gibi okunur ve
önbelleğe alınmaz.

### 8.2 Sistem sesi

Yeni izin yok. macOS 15'ten beri TCC paneli "Screen & System Audio Recording" adını taşıyor,
ve README bu paneli zaten bu adla anıyor. `SCStreamConfiguration.capturesAudio` bu iznin
altındadır; mikrofon istemi **çıkmaz**.

`excludesCurrentProcessAudio` açılır: Snapdeck'in kendi sesi kaydedilmez. Kayıt sırasında hata
sesi çalan bir uygulamanın kendini kaydetmesi geri besleme döngüsüdür.

### 8.3 Mikrofon

Ayrı bir TCC servisi. İki şey gerekir:

1. `apps/desktop/src-tauri/Info.plist` (yeni dosya) içinde `NSMicrophoneUsageDescription`.
   tauri-bundler bu dosyayı ürettiği Info.plist ile birleştirir (kaynak 9). Anahtar eksikse
   macOS süreci ilk mikrofon erişiminde öldürür; bu bir uyarı değil, çökmedir.
2. İstemin tetiklendiği an: kullanıcı ayarlardan mikrofonu açıp **ilk kaydı başlattığında**.
   Ayarlarda kutuyu işaretlemek istem çıkarmaz, çünkü v1 kuralı izin ile kullanım arasına mesafe
   koymuyor.

Reddedilirse: kayıt mikrofonsuz devam eder mi, yoksa hiç başlamaz mı? Karar **hiç başlamaz**
değil, **mikrofonsuz başlar ve bunu söyler**: kullanıcı kaydı başlatmaya karar vermiş, elinde
sessiz bir kayıt olması hiç kayıt olmamasından iyidir, ama sesli sandığı bir dosyayı sessiz bulması
kabul edilemez. Derin bağlantı:
`x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone`.

### 8.4 Ad-hoc imza tuzağı

README, ad-hoc imza yüzünden her güncellemeden sonra ekran kaydı izninin yeniden istendiğini
söylüyor. **Mikrofon izni için de aynısı geçerli** ve aynı yerde yazılmalı: TCC izni imzaya
bağlıdır, ad-hoc imza her derlemede farklıdır, macOS için güncellenmiş Snapdeck başka bir
uygulamadır.

## 9. Arayüz

### 9.1 Başlatma

Menü çubuğuna, mevcut çekim öğelerinin altında kendi grubunda üç öğe: `Record Region`,
`Record Window`, `Record Full Screen`. macOS 14'te üçü de devre dışı ve nedenini söyleyen bir
satır taşır.

Seçim overlay'i aynen kullanılır. Overlay bugün her display için donmuş kare gösteriyor ve
seçimi orada yaptırıyor; kayıt için de doğru davranış budur, çünkü kullanıcı hareketli içerikte
kenar yakalamaya çalışmaz. `Enter`'da fark şu: çekim ikinci bir kare almaz, kayıt ise akışı
overlay kapandıktan **sonra** başlatır ve seçimi `sourceRect` olarak verir.

Overlay'in mevcut kısıtları kayıt için de geçerlidir ve zaten belgelidir: tam ekran Space'lerde
overlay görünmüyor, ve donmuş kareler `~/Library/Caches`'e yazılıyor.

### 9.2 Kayıt sürerken

- **Menü çubuğu başlığı** geçen süreyi gösterir: bir saatin altında `m:ss`, üstünde `h:mm:ss`.
  Gösterilen süre duvar saatidir (`Instant`), `recorded_duration()` değil: duvar saati düzgün
  akar, dosyanın sayacı takılabilir. `recorded_duration()` yalnız özet ve sağlık kontrolü için
  okunur.
- **Başlık sahipliği kuralı.** Tepsi başlığını bugün `FAILURE_MARKER` kullanıyor. Kayıt
  sürerken başlık sayacındır; o sırada oluşan bir hata yalnız tooltip'e ve menü satırına yazılır.
  Kayıt bitince başlık serbest kalır ve bekleyen hata işareti varsa geri döner.
- **Menü**, kayıt sürerken: üç `Record …` öğesi ve üç `Capture …` öğesi devre dışı; yerlerine
  `Stop Recording` ve `Cancel Recording`. Aynı anda tek kayıt, ve kayıt sürerken çekim yok:
  çekim, overlay açıp ekranı dondurur ve o donmuş overlay kaydın içine girerdi.
- **Uyarılar** (disk azalıyor, kare düşüyor) tooltip'e ve menü satırına yazılır, başlığı ele
  geçirmez.

### 9.3 Durdurma ve iptal

`Stop Recording`: akış durur, dosya sonlandırılır (bölüm 6.4 sırası), geçici dosya hedef adına
rename edilir, Recent Captures güncellenir. `Cancel Recording`: aynı sıra, sonra geçici dosya ve
ad rezervasyonu silinir; diskte hiçbir iz kalmaz.

### 9.4 Kayıt bitince ne olmaz

- **Panoya kopyalanmaz.** `tauri-plugin-clipboard-manager` metin ve görüntü yazar, dosya
  yazmaz; bir video yolu metin olarak panoya konsa çoğu uygulamaya yapıştırıldığında ham yol
  düşer. Kullanıcıya hiçbir şey vaat etmemek, yanlış şey vaat etmekten iyidir. README bunu açıkça
  söyler.
- **Editör açılmaz.** `open_editor_after_capture` ayarı kayıtlara uygulanmaz; `packages/editor`
  bir PNG üzerinde çalışır ve video onun işi değildir.

### 9.5 Çökme

`SCRecordingOutput` MP4'ü ilerlemeli yazar ve moov atom'unu sonlandırmada koyar. Süreç kayıt
sırasında ölürse dosya oynatılamaz ve kurtarılamaz; `movieFragmentInterval` gibi bir kaçış
`SCRecordingOutput`'ta yok.

Bunun kullanıcıya maliyeti şöyle sınırlanır: kayıt **hedef klasörde**, başında nokta olan bir
geçici ada yazılır (`.snapdeck-<rastgele>.mp4`), ve yalnız sonlandırma başarılı olursa gerçek
ada rename edilir. Üç sonucu var:

- Aynı birimde olduğu için rename atomiktir ve kopya maliyeti yoktur. Hedef klasör başka bir
  diskteyse bile doğru davranır, çünkü geçici dosya zaten orada.
- Çökme, kullanıcının klasöründe Finder'ın gizlediği tek bir dosya bırakır; `~/Pictures` içinde
  bozuk bir `Snapdeck 2026-09-14 at 10.12.33.mp4` bırakmaz.
- Artıklar iki yerde süpürülür: uygulama açılışında ve her yeni kayıt başlarken. Açılışta da
  süpürülmesi şart, çünkü `RunEvent::Exit` bir çökmede çalışmaz.

Ad çakışması, hareketsiz çekimdeki kuralın aynısıyla çözülür: nihai ad `create_new` ile sıfır
baytlık bir yer tutucu olarak **önceden** kapatılır, kayıt geçici dosyaya yazılır, rename yer
tutucunun üstüne oturur. Böylece hem "hiçbir dosyanın üstüne yazılmaz" garantisi korunur hem
rename atomik kalır.

## 10. Mevcut mimariye oturma

### 10.1 Yeni crate gerekmiyor

Kayıt, `crates/capture`'ın macOS implementasyonunun bir parçasıdır. Ayrı bir `crates/record`
düşünüldü ve elendi: o zaman ya bölüm 6.3'teki hedef çözümleme iki kez yazılır (iki kural sessizce
ayrışır), ya da `SCContentFilter` crate sınırını geçer ve `crates/capture`'ın bugün sıfır olan
ScreenCaptureKit sızıntısı açılır.

Bunun karşılığında README'deki bir cümle düzeltilir. `crates/capture` için "no Tauri, no windows,
no files" yazıyor; doğrusu "no Tauri, no windows; yazdığı tek dosya, bir kaydın kendisine yol
olarak verildiği dosyadır" olur. Crate hâlâ kaydetme klasörünü, ad şablonunu ve Recent Captures'ı
bilmez.

### 10.2 Dokunulan dosyalar

| Dosya | Değişiklik | Aşama |
|---|---|---|
| `crates/frame/src/types.rs` | `RecordingOptions`, `AudioSources`, `RecordingProgress`, `RecordingSummary` | 1, 2 |
| `crates/frame/src/error.rs` | `CaptureError::Unsupported(String)` | 1 |
| `crates/capture/src/lib.rs` | `record()` varsayılan gövdeli; `RecordingSession` trait'i | 1 |
| `crates/capture/src/macos/mod.rs` | `resolve()` çıkarılır, `capture()` onu kullanır | 1 |
| `crates/capture/src/macos/record.rs` | **yeni**: oturum, sayaç handler'ı, durdurma sırası | 1 |
| `crates/capture/Cargo.toml` | `screencapturekit` özelliği `macos_15_0` | 1 |
| kök `Cargo.toml` | aynı özellik satırı | 1 |
| `apps/desktop/src-tauri/src/recording.rs` | **yeni**: oturum sahibi, zamanlayıcı, disk politikası, geçici dosya ve rename | 1 |
| `apps/desktop/src-tauri/src/tray.rs` | kayıt öğeleri, kayıt durumu, başlık sahipliği | 1 |
| `apps/desktop/src-tauri/src/state.rs` | etkin kayıt yuvası, çekimle karşılıklı dışlama | 1 |
| `apps/desktop/src-tauri/src/overlay.rs` | mod dizesine kayıt niyeti eklenir | 1 |
| `apps/desktop/src-tauri/src/commands.rs` | `start_recording`; `capture_region` **değişmez** | 1 |
| `apps/desktop/src-tauri/src/output.rs` | `reserve_recording_path`, `suffixed_file_name` yeniden kullanılır | 1 |
| `apps/desktop/src-tauri/Cargo.toml` | `libc` (boş alan); sonra `image/gif`, `objc2-av-foundation` | 1, 3 |
| `.github/workflows/release.yml` | DMG boyut tavanı kontrolü (bölüm 13) | 1 |
| `apps/desktop/src-tauri/src/settings.rs` | `record_system_audio`, `record_microphone` | 2 |
| `apps/desktop/src-tauri/Info.plist` | **yeni**: `NSMicrophoneUsageDescription` | 2 |
| `apps/desktop/src-tauri/src/export.rs` | **yeni**: trim ve GIF dışa aktarma | 3 |
| `apps/desktop/src/trim.html` + `apps/desktop/src/trim/` | **yeni**: trim penceresi | 3 |
| `apps/desktop/src-tauri/capabilities/trim.json` | **yeni**: pencereye özel ACL | 3 |
| `README.md`, `CHANGELOG.md` | "No screen recording" satırı düşer, yerine ne var ne yok yazılır | her aşama |

### 10.3 Değişmeyenler

`capture_region`, `save_edited`, `copy_edited`, izin komutları, köprü, `crates/stitch`,
`packages/protocol`, `apps/extension`. Kayıt bunların hiçbirinin yolundan geçmez.

### 10.4 `packages/editor` ve trim

Editör değişmez. Trim, `packages/editor`'ün içine girmez ve `<Editor/>` bileşenini kullanmaz:
editör bir katman modeli, bir undo yığını ve bir canvas renderer'ıdır, bunların hiçbiri bir video
aralığı için anlamlı değildir. Trim penceresi, bir `<video>` elemanı, iki tutamak ve iki düğmeden
oluşan ayrı ve küçük bir penceredir; kendi capability dosyasıyla gelir ve yalnız kendi komutlarına
erişir.

Aynı gerekçeyle trim, editörün "rename ile değiştir" davranışını **taklit eder**: `Save`
dosyayı yerine rename ile yazar, `Close` diskteki dosyayı olduğu gibi bırakır. Kullanıcı bu
sözleşmeyi hareketsiz çekimlerden zaten biliyor.

### 10.5 Ad şablonu ve Recent Captures

- `render_filename` aynen kullanılır. `{width}` ve `{height}` kaydın piksel boyutudur,
  `{date}`/`{time}` kaydın **başladığı** andır (bittiği an değil: kullanıcı kaydı başlattığı anı
  hatırlar).
- Uzantı formatın kendisidir: `.mp4` veya `.gif`. `check_filename_template`'in iki kuralı
  (uzantının önünde bir ad olmalı, ve en geniş ikamede 255 baytı aşmamalı) bu uzantılarla da
  doğrulanır.
- `recents::record(app, path)` değişmeden çalışır; liste yol tutar, tür tutmaz. Tepsi menüsünde
  bir MP4 ile bir PNG aynı görünür ve ikisi de Finder'da gösterilir. `RECENT_CAPTURE_LIMIT` beş
  olarak kalır.

## 11. Hata yönetimi

v1 tablosunun devamı; oradaki satırlar yürürlükte kalır.

| Durum | Davranış |
|---|---|
| macOS 14, kayıt istendi | Menü öğesi zaten devre dışı. Yine de ulaşılırsa `Unsupported` döner ve "ekran kaydı macOS 15 gerektirir" denir. |
| Ekran kaydı izni yok | Kayıt başlamaz; v1'in yönlendiren modali ve derin linki. Sessizce boş dosya yok. |
| Mikrofon izni reddedildi | Kayıt mikrofonsuz devam eder ve bu açıkça söylenir (bölüm 8.3). |
| Kayıt sırasında hedef pencere kapandı | `SCStream` delegate'i hatayla durur; dosya sonlandırılır ve elde olan kadarı kaydedilir, kullanıcıya neden kısa olduğu söylenir. |
| Kodlayıcı yetişemiyor | Kare düşer, video kısalmaz. Sayaç gerçekleşen fps'i belirgin düşük görürse tooltip'te söylenir. |
| Disk azalıyor | Bölüm 7.2'nin üç eşiği: başlatma reddi, uyarı, düzgün durdurma. |
| Sonlandırma başarısız | Geçici dosya silinir ve rename yapılmaz. Kullanıcı bozuk bir dosyayla kalmaz; hiçbir dosyayla kalır ve nedenini okur. |
| Kayıt sırasında çökme | Hedef klasörde gizli bir geçici dosya kalır; açılışta ve sonraki kayıtta süpürülür. Kurtarma iddiası yok. |
| Kayıt sürerken çekim tetiklendi | Reddedilir ve nedeni söylenir. Overlay açılsa donmuş ekran kaydın içine girerdi. |
| Trim dışa aktarımı başarısız | Diskteki kayıt olduğu gibi kalır. Düzenlenmemiş kayıt hiçbir koşulda kaybolmaz. |
| GIF dışa aktarımı başarısız | MP4 geçici dosyası korunur ve kullanıcıya MP4 olarak kaydetme yolu sunulur. |

## 12. Test stratejisi

v1 kuralı sürüyor: **mutasyon kanıtı olmayan test yazılmaz.** Her test için, testi geçiren kodun
hangi tek satırı bozulunca testin kırmızıya döneceği yazılır ve testi yazan kişi o satırı gerçekten
bozup kırmızıyı görmekle yükümlüdür.

### 12.1 Saf ve deterministik test edilebilenler

| Ne | Nerede | Mutasyon kanıtı |
|---|---|---|
| fps to `CMTime` | `crates/capture` | `CMTime { value: 1, timescale: fps }` yerine `{ value: fps, timescale: 1 }` yazılınca 30 fps testi kırılır |
| Durdurma sırası (bölüm 6.4) | `crates/capture`, sahte bir `StopSequence` ile | `try_remove_output_handler` ile `remove_recording_output` satırları yer değiştirince sıra testi kırılır |
| `disk_verdict` | `apps/desktop` | `STOP_SECONDS_LEFT` karşılaştırması `<` yerine `<=` ya da eşik `WARN` ile takas edilince sınır testleri kırılır |
| `RESERVE_BYTES` düşülmesi | `apps/desktop` | Çıkarma kaldırılınca "rezervi harcamaz" testi kırılır |
| Geçen süre biçimi | `apps/desktop` | `3600` sınırında `h:mm:ss`'e geçiş kaldırılınca `1:00:00` testi `60:00` verir ve kırılır |
| Kayıt dosya adı, uzantı ve çakışma soneki | `apps/desktop/src-tauri/src/output.rs` | `suffixed_file_name`'in ilk denemede sonek koymaması bozulunca "ilk kayıt adını korur" testi kırılır |
| Yer tutucu + rename sözleşmesi | `apps/desktop`, gerçek geçici dizinde | `create_new` yerine `create` yazılınca "var olan dosyanın üstüne yazmaz" testi kırılır |
| İptalin iz bırakmaması | `apps/desktop`, gerçek geçici dizinde | Yer tutucunun silinmesi atlanınca "iptal sonrası dizin boş" testi kırılır |
| Oturum durum makinesi | `apps/desktop` | `Recording` durumunda çekim isteğini reddeden kol kaldırılınca karşılıklı dışlama testi kırılır |
| Trim aralığı aritmetiği | `apps/desktop/src-tauri/src/export.rs` | `start < end` kontrolü ya da süreye kırpma kaldırılınca ters/taşan aralık testleri kırılır |
| GIF zaman çizelgesi ve santisaniye dağıtımı | `apps/desktop/src-tauri/src/export.rs` | Kalan biriktirmesi kaldırılıp her gecikme `round()`'a düşürülünce "30 saniyelik GIF 30 saniye sürer" testi kırılır |
| `Unsupported` varyantının serde etiketi | `crates/frame` | `rename_all = "camelCase"` kaldırılınca frontend'in beklediği `"unsupported"` testi kırılır |
| Varsayılan `record()` gövdesi | `crates/capture`, `MockCapturer` üzerinden | Varsayılan `Err(Unsupported)` yerine `Ok` döndürülünce "kayıt desteklemeyen capturer sessizce başarı demez" testi kırılır |

### 12.2 Gerçek donanım isteyenler

Bunlar birim testi değildir ve CI'da çalışmaz: GitHub'ın macOS runner'larında ekran kaydı TCC
izni yoktur, `SCShareableContent::get()` başarısız olur. Sürüm öncesi elle çalıştırılan bir
kontrol listesi olarak tutulur ve sonuçları CHANGELOG'a sayıyla yazılır.

- Üç hedefte kayıt başlıyor, duruyor, iptal ediliyor; çıkan MP4 QuickTime'da açılıyor.
- Retina'da çıktı piksel boyutu, seçilen nokta boyutunun iki katı.
- Yük altında (4K, 60 fps, hareketli içerik) kare düşme davranışı ve gerçekleşen fps.
- Sistem sesi kaydın içinde; Snapdeck'in kendi sesi değil.
- **Mikrofon ölçümü** (aşama 2b kapısı): `captureMicrophone = true` ile üretilen MP4 oynatılıyor mu.
- Passthrough trim'in keyframe granülerliği: baş kesme gerçekte kaç milisaniye kayıyor.
- 10 saniyelik klipten GIF üretme süresi ve dosya boyutu.
- Disk dolarken üç eşiğin de gerçekten tetiklenmesi.
- Kayıt sırasında süreç `kill -9` edildiğinde hedef klasörde ne kalıyor ve açılışta süpürülüyor mu.

## 13. Bundle boyutu

Bugün DMG 5,5 MB. Seçilen yolun eklediği:

| Kalem | Tahmin | Gerekçe |
|---|---|---|
| `SCRecordingOutput` kullanımı | 0 MB | Sistem framework'ü; `screencapturekit` zaten linkli, yalnız bir özellik bayrağı açılıyor |
| Kayıt Rust kodu (aşama 1 + 2) | 0,1-0,3 MB | ~600-800 satır, LTO ve strip sonrası |
| `libc` | ~0 MB | `Cargo.lock`'ta zaten var, tek fonksiyon (`statfs`) |
| `image` `gif` özelliği (aşama 3) | 0,1-0,2 MB | `gif` 0.13 + `color_quant` 1.1 |
| `objc2-av-foundation` (aşama 3) | 0,1-0,2 MB | Bağlama crate'i; framework sistemin |
| Trim penceresi (HTML/JS) | ~0,05 MB | Üç dosya, mevcut bundle'a girer |
| **Toplam** | **+0,35 ile +0,75 MB** | |

**Önerilen tavan: 7,0 MB DMG.** Yani bugünkü 5,5'in üstünde 1,5 MB, yaklaşık %27 pay. Tahminin
iki katından fazla bir marj bırakıyor ve yine de "lightweight" cümlesini savunulabilir tutuyor.

Tavan bir cümle olarak kalırsa hiçbir şey ifade etmez. Bu yüzden `release.yml`, DMG'yi ölçen ve
tavanı aşarsa sürümü **başarısız kılan** bir adım kazanır; sayı workflow'da tek bir yerde,
gerekçesiyle birlikte durur. Tavanı yükseltmek, o satırı ve gerekçesini değiştirmeyi gerektirir,
yani bilinçli bir karar olur.

## 14. Açık kalan kararlar

Üçü de ürün sahibinin kararıdır; tasarım her biri için bir öneri taşıyor ama kararı vermiyor.

**S1. Kayıt macOS 15 isterken uygulamanın tabanı 14'te mi kalsın?**
Öneri: **evet, 14'te kalsın.** Tabanı 15'e çekmek, ekran görüntüsü için macOS 14 kullanıcılarını
hiçbir kazanç olmadan dışarıda bırakır; kayıt zaten çalışma zamanında kapanıyor. Bedeli, menüde
devre dışı üç öğe ve README'de bir paragraf.

**S2. Passthrough trim'in keyframe kayması kabul edilebilir mi?**
Öneri: **önce ölçülsün, sonra karar verilsin.** Ölçülen kayma 1 saniyenin altındaysa passthrough
kalır ve sayı README'ye yazılır. 2 saniyeyi aşarsa, trim için yeniden kodlama seçeneği tartışılır;
bu, kalite kaybı ve on saniyelerce bekleme demektir ve bu yüzden varsayılan olamaz.

**S3. Mikrofon ölçümü kötü çıkarsa aşama 2b ne olsun?**
Öneri: **mikrofon bu sürüme alınmasın**, ayrı AAC + mux yoluna (bölüm 5.4, seçenek 1)
girilmesin. Sebep: o yol, bölüm 5.2'de yazmamak için karar verdiğimiz `AVAssetWriter` işinin
ses yarısını geri getirir, ve aşama 2a tek başına zaten kullanılabilir bir üründür. Sistem sesi
ile yayınlanıp mikrofon bir sonraki sürüme bırakılabilir.

## 15. Ölçülen gerçekler ve kaynakları

Bu dokümandaki her sürüm, tarih ve API iddiası aşağıdakilerden birine dayanır. Ezberden yazılan
hiçbir crate adı yoktur.

1. `screencapturekit` **9.0.1**, yayın **2026-08-31**, lisans MIT OR Apache-2.0. Depodaki sürüm
   (`Cargo.lock`). crates.io'daki güncel sürüm **10.0.3** (2026-09-07); bu tasarım 9.0.1 ile
   yazıldı ve 10.x'e geçiş **bu dokümanın kapsamı dışıdır**, çünkü 9.0.1 kaydın ihtiyaç duyduğu
   her şeyi taşıyor.
2. Crate kaynağı okundu: `~/.cargo/registry/src/index.crates.io-*/screencapturekit-9.0.1/src/`
   içinde `recording_output.rs`, `stream/sc_stream.rs`, `stream/configuration/audio.rs`,
   `stream/output_type.rs`, `cm/mod.rs`, `cm/frame_status.rs` ve crate `Cargo.toml`'u.
   `macos_15_0` özelliğinin neyi açtığı ve `remove_recording_output`'un sonlandırma koşulu
   (bölüm 6.4) buradan.
3. `ffmpeg-sidecar` **2.5.2**, **2026-05-30**, MIT (crates.io API).
4. `objc2-av-foundation` **0.3.2**, **2025-10-04**, Zlib OR Apache-2.0 OR MIT; `AVAsset`,
   `AVURLAsset`, `AVAssetExportSession`, `AVAssetImageGenerator`, `AVAssetReader` açık (docs.rs).
   `gifski` **1.34.0**, **2025-07-13**, **AGPL-3.0-or-later**, bu yüzden elendi (crates.io API).
   `gif` güncel **0.14.2** (2026-04-09) MIT OR Apache-2.0, `color_quant` güncel **2.0.0**
   (2026-05-09) MIT; `image` 0.25 bunları 0.13.1 ve 1.1'e sabitliyor (docs.rs özellik sayfası).
   `image` **0.25.10** ve `libc` **0.2.189** `Cargo.lock`'ta zaten var.
5. WWDC24, "Capture HDR content with ScreenCaptureKit":
   `https://developer.apple.com/videos/play/wwdc2024/10088/`. `SCRecordingOutput`'un ekran, ses
   ve mikrofon içeriğini kaydetmenin "basit ve elverişli yolu" olduğu ifadesi.
6. Apple Developer Forums 805892, "ScreenCaptureKit recording output is corrupted when
   captureMicrophone is true": `https://developer.apple.com/forums/thread/805892`. Kasım 2025
   açıldı, Mart 2026 son yanıt, çözülmedi. Bölüm 5.4'ün tamamı buna dayanıyor.
7. `mixesAudioWithMicrophone` iddiası yalnız üçüncü taraf bir GitHub issue'da geçiyor ve Apple
   belgelerinde **doğrulanamadı**. Bu tasarım ona dayanmıyor; burada yalnız kayda geçmesi için var.
8. macOS 27 (Golden Gate) **2026-09-14**'te yayınlandı; öncülü macOS 26 Tahoe (2025), 15 Sequoia
   (2024), 14 Sonoma (2023). Bölüm 4.3'teki sürüm merdiveni buradan.
9. Tauri, macOS Application Bundle: `src-tauri/Info.plist` varsa tauri-bundler onu ürettiği
   Info.plist ile birleştirir. `https://v2.tauri.app/distribute/macos-application-bundle/`.
10. Depo içi ölçümler: DMG bugün 5,5 MB; `tauri.conf.json` `minimumSystemVersion` 14.0; CI ve
    release `macos-latest` üzerinde; `image` 0.25 `default-features = false` ile `png` ve `jpeg`
    açık, `gif` kapalı.

## 16. Verilen kararlar

Bölüm 14'ün üç sorusu, tasarımın önerileri kabul edilerek kapatıldı.

**S1. Uygulamanın tabanı macOS 14'te kalır, kayıt 15 ister.** Tabanı 15'e çekmek, ekran görüntüsü
için macOS 14 kullanıcılarını hiçbir karşılık olmadan dışarıda bırakırdı. Kayıt çalışma zamanında
`SCRecordingOutput::is_available()` ile kapanır.

**S2. Trim passthrough kalır, kayma önce ölçülür.** Aşama 3 keyframe granülerliğini ölçer ve sayıyı
README'ye yazar. Ölçülen kayma 2 saniyeyi aşarsa yeniden kodlama seçeneği o zaman tartışılır.

**S3. Mikrofon ölçümü kötü çıkarsa mikrofon bu sürüme alınmaz.** Ayrı AAC yazıp sonra muxlama
yoluna girilmez: o yol, bölüm 5.2'de yazmamak için karar verdiğimiz `AVAssetWriter` işinin ses
yarısını geri getirir. Aşama 2a tek başına kullanılabilir bir üründür.

Ek karar: `screencapturekit` **9.0.1**'de kalınır. crates.io'da 10.0.3 var, ama 9.0.1 kaydın
ihtiyaç duyduğu her şeyi taşıyor ve major sürüm atlamak ayrı bir iştir.
