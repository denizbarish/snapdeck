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

## 16. Verilen kararlar

Bölüm 14'ün üç sorusu, tasarımın önerileri kabul edilerek kapatıldı.

**S1. Uygulamanın tabanı macOS 14'te kalır, kayıt 15 ister.** Tabanı 15'e çekmek, ekran görüntüsü
için macOS 14 kullanıcılarını hiçbir karşılık olmadan dışarıda bırakırdı. Kayıt çalışma zamanında
`SCRecordingOutput::is_available()` ile kapanır; menüdeki üç kayıt öğesi o makinelerde devre dışı
görünür ve nedenini söyler.

**S2. Trim passthrough kalır, kayma önce ölçülür.** Aşama 3 keyframe granülerliğini ölçer ve sayıyı
README'ye yazar. Ölçülen kayma 2 saniyeyi aşarsa yeniden kodlama seçeneği o zaman tartışılır;
varsayılan olamaz, çünkü kalite kaybı ve on saniyelerce bekleme demektir.

**S3. Mikrofon ölçümü kötü çıkarsa mikrofon bu sürüme alınmaz.** Ayrı AAC yazıp sonra muxlama
yoluna girilmez: o yol, bölüm 5.2'de yazmamak için karar verdiğimiz `AVAssetWriter` işinin ses
yarısını geri getirir. Aşama 2a (sistem sesi) tek başına kullanılabilir bir üründür ve mikrofon
bir sonraki sürüme bırakılır. Yayınlanmış bir Snapdeck'in oynatılamayan MP4 üretmesi kabul
edilemez.

Ek karar: `screencapturekit` **9.0.1**'de kalınır. crates.io'da 10.0.3 var, ama 9.0.1 kaydın
ihtiyaç duyduğu her şeyi taşıyor ve major sürüm atlamak ayrı bir iştir, kaydın önkoşulu değil.
