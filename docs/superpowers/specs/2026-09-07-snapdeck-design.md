# Snapdeck, Tasarım Dokümanı

- Tarih: 2026-09-07
- Durum: onaylandı
- Kapsam: v1 (yakalama çekirdeği + annotation editörü + full-page köprüsü)

## 1. Amaç

Açık kaynak, hafif, gizlilik önceliğli bir ekran görüntüsü uygulaması. CleanShot X ve Shottr
sınıfında yakalama ve düzenleme deneyimi, tamamen yerel çalışma, hesap ve sunucu yok.
Web sayfalarının tamamını (kaydırmalı, full-page) doğru şekilde yakalayabilme, ürünün
diğerlerinden ayrıştığı nokta.

## 2. Kapsam sınırı

### v1 kapsamı
- Bölge, pencere ve tam ekran yakalama (macOS)
- Katmanlı annotation editörü
- Chrome eklentisi ile full-page yakalama + masaüstüne köprü
- Eklenti yokken masaüstü otomatik kaydırma + birleştirme (fallback, beta)
- Tepsi (tray) menüsü, global kısayollar, ayarlar
- Yerel çıktı: dosyaya kaydet, panoya kopyala, sürükle-bırak

### v1 dışı (v2 backlog)
- Ekran kaydı (MP4/GIF, mikrofon ve sistem sesi, trim), en ağır alt sistem olduğu için sonraya alındı.
  v2'de `ScreenCapturer` trait'ine `stream()` metodu eklenir; `Frame` tipi stride, pixel format ve
  timestamp taşıdığı için bu ekleme mevcut çağıranları kırmaz.
- İmleç vurgusu ve tıklama efekti
- Windows ve Linux implementasyonları (soyutlama v1'de hazır, implementasyon sonra)
- Bulut yükleme, paylaşım linki, hesap sistemi (ürün hedefi değil)
- OCR, metin çıkarma

## 3. Teknoloji seçimi ve gerekçe

| Karar | Seçim | Gerekçe |
|---|---|---|
| Masaüstü çerçeve | Tauri v2 (2.9.x) | ~10 MB bundle, düşük RAM. Electron'un ~150 MB'ı "lightweight" hedefiyle çelişiyor. |
| Çekirdek dil | Rust | Yakalama ve görüntü işleme sıcak yolu; native API'lere doğrudan bağlanır. |
| Arayüz | React 19 + TypeScript + Tailwind + shadcn/ui | Mevcut stack, editör için canvas ile iyi çalışıyor. |
| macOS yakalama | `screencapturekit` crate (macOS 13+) | Güvenli, idiomatik binding. `scap` bakımsız; `xcap`'in frame tipinde stride, pixel format ve timestamp yok. |
| Windows yakalama (v2) | `windows-capture` (Windows.Graphics.Capture) | Trait arkasında, üst katman değişmeden takılır. |
| Eklenti | Manifest V3 + TypeScript + Vite/CRXJS | MV3 zorunlu; `captureBeyondViewport` yalnızca CDP'de olduğu için eklenti scroll+stitch yapar. |
| Paket yöneticisi | pnpm workspaces + Cargo workspace | Tek monorepo, iki dil. |
| Lisans | MIT | Katkı ve fork önünde en az sürtünme. |

## 4. Depo yapısı

```
apps/desktop/        Tauri v2 uygulaması (Rust host + React arayüz)
apps/extension/      MV3 Chrome eklentisi
packages/editor/     Annotation editörü, saf TS/React, Tauri bağımsız
packages/protocol/   Köprü mesaj şemaları (zod), iki taraf paylaşır
crates/capture/      Yakalama soyutlaması ve platform implementasyonları
crates/stitch/       Kaydırmalı birleştirme algoritması
docs/                Tasarım, mimari kararlar, kullanıcı belgeleri
```

### Modül sınırları

Her modül tek sorumluluk taşır, arayüzü üzerinden tüketilir, bağımsız test edilir.

- **`crates/capture`**: Ne yapar: ekranları/pencereleri listeler, kare yakalar. Nasıl kullanılır:
  `ScreenCapturer` trait'i. Neye bağlı: platform API'leri. Tauri'yi bilmez.
- **`crates/stitch`**: Ne yapar: örtüşen kare dizisini tek görüntüye birleştirir. Nasıl kullanılır:
  `stitch(frames: &[Frame], hint: ScrollHint) -> Result<Image>`. Neye bağlı: yalnızca görüntü verisi.
  Ekran, pencere veya dosya sistemi bilmez, bu yüzden sentetik girdilerle deterministik test edilir.
- **`packages/editor`**: Ne yapar: görüntü + katman listesini alır, düzenlenmiş görüntü üretir.
  Nasıl kullanılır: `<Editor image={...} onExport={...} />` ve saf `EditorCore` sınıfı.
  Neye bağlı: hiçbir Tauri API'si yok. Bu izolasyon sayesinde eklenti de aynı editörü kullanabilir.
- **`packages/protocol`**: Ne yapar: köprü mesajlarını tanımlar ve doğrular. Tek gerçek kaynağı;
  masaüstü ve eklenti aynı şemayı import eder, sürüm uyuşmazlığı çalışma zamanında yakalanır.

## 5. Yakalama çekirdeği

### 5.1 Soyutlama

```rust
pub trait ScreenCapturer {
    fn displays(&self) -> Result<Vec<DisplayInfo>>;
    fn windows(&self) -> Result<Vec<WindowInfo>>;
    fn capture(&self, target: CaptureTarget) -> Result<Frame>;
}

pub struct Frame {
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub stride: usize,
    pub pixel_format: PixelFormat,
    pub scale_factor: f32,   // Retina ve karışık DPI için zorunlu
    pub timestamp: Instant,
}
```

`scale_factor` frame ile birlikte taşınır, sonradan tahmin edilmez. Çoklu monitörde farklı DPI'lar
birleştirme ve dışa aktarma aşamasında bu değere göre normalize edilir.

### 5.2 Seçim overlay'i

Global kısayol tetiklendiğinde:

1. Tüm ekranların anlık kareleri yakalanır.
2. Her monitör için ayrı şeffaf, always-on-top pencere açılır; arka planında donmuş kare gösterilir.
   Canlı ekran gösterilmez, böylece imleç titremesi ve geri besleme döngüsü olmaz.
3. Seçim arayüzü: piksel hassas dikdörtgen, kenar tutamaçları, klavye ile ince ayar (ok tuşları),
   boyut göstergesi, magnifier ve piksel renk kodu (kopyalanabilir).
4. Pencere modunda `CGWindowList` ile pencere dikdörtgenleri okunur, imleç altındaki pencereye snap edilir.
5. `Esc` iptal eder, tüm overlay pencereleri kapanır.

### 5.3 İzinler

macOS ekran kaydı izni (TCC) ilk yakalamada istenir. Reddedilirse uygulama sessizce boş kare
üretmez; yönlendiren bir modal açar ve Sistem Ayarları'ndaki ilgili panele derin link verir.
İzin durumu her yakalama öncesi kontrol edilir, kullanıcı izni sonradan geri alabilir.

## 6. Editör

### 6.1 Model

Katmanlı nesne modeli. Her annotation bir veri nesnesidir, piksele pişirilmez; sonradan seçilir,
taşınır, düzenlenir, silinir.

```ts
type Layer =
  | { kind: 'arrow';   from: Point; to: Point; style: StrokeStyle }
  | { kind: 'rect' | 'ellipse'; rect: Rect; style: ShapeStyle }
  | { kind: 'line';    points: Point[]; style: StrokeStyle }
  | { kind: 'text';    rect: Rect; content: string; style: TextStyle }
  | { kind: 'highlight'; rect: Rect; color: Color }
  | { kind: 'obscure'; rect: Rect; mode: 'blur' | 'pixelate' | 'blackout'; intensity: number }
  | { kind: 'step';    center: Point; index: number; style: BadgeStyle }
```

Kırpma katman değil, belge seviyesinde bir `crop: Rect` alanıdır.

### 6.2 Render

Canvas2D yeterlidir; WebGL bu iş yükü için erken optimizasyon olur. Render saf fonksiyondur:
`render(image, layers, crop) -> ImageBitmap`. Aynı fonksiyon hem ekranda hem dışa aktarmada kullanılır,
böylece önizleme ile çıktı ayrışamaz.

### 6.3 Undo/redo

Komut yığını (`Command { apply, invert }`). Katman ekleme, taşıma, stil değiştirme, silme birer komut.
Yığın saf veridir, testi Vitest ile birim düzeyinde yapılır.

### 6.4 Gizlilik kritik davranış

`obscure` katmanı dışa aktarmada gerçekten pikselleri yok eder: kaynak görüntü bölgesi bulanıklaştırılıp
düzleştirilir, orijinal pikseller çıktı dosyasında hiçbir biçimde (katman verisi, meta veri, EXIF) yer almaz.
`blackout` modu geri döndürülemez düz dolgu üretir.

### 6.5 Çıktı

PNG (varsayılan), JPEG, WebP. Panoya kopyala, dosyaya kaydet (yapılandırılabilir klasör ve ad şablonu),
editör penceresinden dışarı sürükle-bırak.

## 7. Full-page yakalama

### 7.1 Eklenti yolu (birincil)

1. Content script belge yüksekliğini, `devicePixelRatio` değerini ve kaydırma davranışını ölçer.
2. `position: sticky` ve `position: fixed` elemanlar tespit edilir. İlk katman dışındaki her katmanda
   gizlenir; aksi halde sabit başlık her katmana damgalanır.
3. Viewport yüksekliği kadar kaydırılır, her adımda `chrome.tabs.captureVisibleTab` çağrılır.
   Chrome saniyede 2 çağrı sınırı uygular; çağrılar bu sınıra göre throttle edilir.
4. Katmanlar offscreen canvas'ta birleştirilir, DPR'a göre ölçek düzeltmesi uygulanır
   (DPR 2'de 400x300 CSS pikseli 800x600 bitmap'tir, naif birleştirme bulanık çıktı üretir).
5. Sonuç PNG olarak köprüye verilir.

Bilinen sınır: lazy-load ile geç yüklenen görseller ve sanal listeler (virtualized list) eksik çıkabilir.
Katman aralarında kısa bekleme ve `scroll` olayı tetiklemesi uygulanır, ancak garanti verilmez ve
kullanıcıya bu durum belgelenir.

### 7.2 Köprü

Masaüstü uygulama `127.0.0.1` üzerinde WebSocket sunucusu dinler. Native messaging yerine loopback
WebSocket seçildi: tek kurulum adımı, tarayıcıdan bağımsız, host manifest dosyası gerektirmiyor.

Güvenlik kuralları:
- Yalnızca loopback arayüzüne bağlanır, dış ağdan erişilemez.
- Eşleştirme token'ı: uygulama ilk açılışta rastgele token üretir, kullanıcı eklenti ayarlarına yapıştırır.
  Token'sız veya yanlış token'lı bağlantı reddedilir.
- `Origin` başlığı doğrulanır, yalnızca eklenti kaynağı kabul edilir.
- Mesaj boyutu sınırlıdır, `packages/protocol` şemasıyla doğrulanır, doğrulanmayan mesaj bağlantıyı kapatır.

Uygulama kapalıysa eklenti bağlanamaz ve normal dosya indirmeye düşer; kullanıcıya bu durum bildirilir.

### 7.3 Masaüstü fallback

Eklenti kullanılmadığında: hedef pencerede sentetik kaydırma olayı üretilir, her adımda kare yakalanır,
`crates/stitch` faz korelasyonu ile örtüşme miktarını tespit edip birleştirir. Her kaydırılabilir
uygulamada çalışır, ancak sabit başlıkları ayırt edemez. Arayüzde açıkça "deneysel" etiketlenir.

## 8. Uygulama kabuğu

- Tepsi (tray) menüsü: yakalama eylemleri, son çekimler, ayarlar, çıkış.
- Global kısayollar: `tauri-plugin-global-shortcut`, kullanıcı tarafından yeniden atanabilir,
  çakışma tespiti yapılır.
- Ayarlar: `tauri-plugin-store` ile JSON dosyasında; kayıt klasörü, ad şablonu, format, kısayollar,
  köprü token'ı, otomatik başlatma.
- Otomatik güncelleme: `tauri-plugin-updater`, GitHub Releases üzerinden.

## 9. Hata yönetimi

| Durum | Davranış |
|---|---|
| Ekran kaydı izni yok | Yönlendiren modal + Sistem Ayarları derin linki. Sessiz boş kare asla üretilmez. |
| Yakalama başarısız | Kullanıcıya toast, ayrıntı log dosyasına; overlay temiz kapanır, ekranda takılı kalmaz. |
| Köprü bağlantısı yok/koptu | Eklenti dosya indirmeye düşer, kullanıcı bilgilendirilir. |
| Köprü şema uyuşmazlığı | Bağlantı kapatılır, sürüm uyuşmazlığı mesajı gösterilir. |
| Disk dolu / yazma hatası | Kaydetme hatası bildirilir, görüntü bellekte kalır ve panoya kopyalanabilir. |
| Karışık DPI çoklu monitör | Her frame kendi `scale_factor` değeriyle taşınır, birleştirmede normalize edilir. |
| Birleştirmede örtüşme bulunamadı | Kısmi sonuç kullanıcıya sunulur ve "eksik olabilir" uyarısı verilir; sessizce bozuk görüntü üretilmez. |

## 10. Test stratejisi

- **Rust birim:** `crates/stitch` sentetik, deterministik görüntülerle (bilinen örtüşme, sticky şeridi,
  örtüşmesiz kenar durumu). `crates/capture` trait'i mock implementasyonla; platform API'si test edilmez.
- **TS birim (Vitest):** editör komut yığını (undo/redo değişmezleri), hit-testing, geometri,
  export render'ının determinizmi, `obscure` katmanının çıktıda orijinal pikselleri sızdırmaması.
- **Eklenti (Playwright):** sabit başlık, lazy-load görsel ve uzun içerik barındıran fixture sayfada
  full-page yakalama; sonuç boyutu ve sticky damgalanmaması doğrulanır.
- **Uçtan uca:** `tauri-driver` ile duman testi (uygulama açılır, kısayol tetiklenir, editör açılır).
- **CI:** `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`, `pnpm lint`, `pnpm test`.

## 11. Açık kaynak ve dağıtım

- MIT lisans, README, CONTRIBUTING, issue ve PR şablonları, davranış kuralları.
- GitHub Actions: PR'da lint + test; tag'de `tauri-action` ile build ve release.
- **İmzalama uyarısı:** Apple Developer ID (yıllık ücretli) olmadan notarization yapılamaz; imzasız
  build'ler kullanıcıda Gatekeeper uyarısı verir. README'de açıkça belgelenir, sürüm notlarında tekrarlanır.
- Docker yalnızca CI ve Linux build container'ı için kullanılır; masaüstü uygulamanın çalışma zamanı
  container'ı yoktur.

## 12. Fazlar

| Faz | İçerik | Bitti kriteri |
|---|---|---|
| 0 | Monorepo iskeleti, CI, Tauri kabuğu, tray, global kısayol | Kısayol basıldığında boş overlay açılıyor, CI yeşil |
| 1 | `crates/capture` macOS + seçim overlay'i + izin akışı | Bölge/pencere/tam ekran yakalanıp PNG olarak kaydediliyor |
| 2 | `packages/editor` + editör penceresi + çıktı yolları | Yakalama sonrası editör açılıyor, annotate edilip kopyalanıyor/kaydediliyor |
| 3 | Eklenti + `packages/protocol` + köprü + `crates/stitch` fallback | Eklentiden full-page yakalama masaüstü editörde açılıyor |
| 4 | Ayarlar, auto-update, belgeler, ilk release | Genel kullanıma açık v0.1.0 release |

v2 backlog (bu spec'in dışında, ayrı spec yazılacak): ekran kaydı, imleç efektleri, Windows desteği.
