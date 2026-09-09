# Snapdeck Editor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Yakalanan görüntü, kaydedildikten sonra bir editör penceresinde açılır; kullanıcı ok, kutu, elips, serbest çizgi, metin, vurgu, gizleme ve adım numarası ekler, kırpar, geri alır ve sonucu kaydeder veya panoya kopyalar.

**Architecture:** Editörün tamamı `packages/editor` içinde, Tauri'den bağımsız saf TypeScript ve React olarak durur; masaüstü uygulaması ona yalnızca bir görüntü ve bir çıktı geri çağrısı verir. Katmanlar veri nesnesidir, piksele pişirilmez; render saf bir fonksiyondur ve aynı fonksiyon hem ekranı hem dışa aktarmayı üretir, böylece önizleme ile çıktı ayrışamaz. Geri alma, tersine çevrilebilir komut yığınıdır.

**Tech Stack:** TypeScript (strict, `noUncheckedIndexedAccess`), React 19, Canvas2D, Vitest, Tauri 2.11.5.

## Global Constraints

- Hedef platform macOS 14.0+. Kod, yorum, commit mesajı ve arayüz metinleri İngilizce; yalnızca `docs/superpowers/` Türkçe.
- Ağ çağrısı yok, telemetri yok.
- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `pnpm lint`, `pnpm test`, `pnpm build` her commit'te temiz.
- `packages/editor` **Tauri API'si import etmez.** Tek bağımlılığı React'tir. Bu, full-page eklentisinin aynı editörü kullanabilmesi için zorunludur ve testlerin tarayıcı ya da masaüstü olmadan koşmasını sağlar.
- Render saf fonksiyondur: `renderDocument` ekranda ve dışa aktarmada aynı kodu çalıştırır. İki ayrı çizim yolu yazmak yasaktır.
- **Gizlilik:** `obscure` katmanı dışa aktarmada gerçekten pikselleri yok eder. Çıktı dosyasında orijinal pikseller hiçbir biçimde (katman verisi, meta veri, EXIF) yer almaz.
- Yakalama akışı bozulmaz: çekim bugünkü gibi dosyaya yazılır ve panoya kopyalanır, editör bunun üstüne açılır. Editörden kaydetmek aynı dosyayı günceller.

## Bu planın kapsamı dışında

- Sürükle-bırak ile dışa aktarma (ek bağımlılık, düşük değer).
- WebGL render (Canvas2D bu iş yükü için yeterli).
- Ekran kaydı (kullanıcı kararıyla v2).
- Ayarlar arayüzü, kısayol yeniden atama, imzalama, otomatik güncelleme (Plan 4).

## Plan formatı hakkında

Bu plan tip tanımlarını, fonksiyon imzalarını ve test durumlarını **birebir** verir: sözleşme budur,
uygulayıcı bunları değiştiremez. Gövde kodunu uygulayıcı yazar. Sebebi, önceki planın gövde kodunun
incelemelerde defalarca değişmiş olması; sözleşmeyi sabitleyip uygulamayı serbest bırakmak aynı kaliteyi
daha az sürtünmeyle veriyor. İnceleme kapısı her görevde aynen işler.

---

## File Structure

```
packages/editor/package.json
packages/editor/tsconfig.json
packages/editor/src/index.ts              dışa açılan yüzey
packages/editor/src/model.ts              Layer, EditorDocument, saf yardımcılar
packages/editor/src/commands.ts           komut yığını, undo/redo
packages/editor/src/hit.ts                seçim, taşıma, yeniden boyutlandırma geometrisi
packages/editor/src/render.ts             renderDocument, tek çizim yolu
packages/editor/src/export.ts             toBlob, obscure düzleştirme
packages/editor/src/Editor.tsx            React arayüzü
packages/editor/src/*.test.ts             Vitest

apps/desktop/editor.html                  editör penceresi girişi
apps/desktop/src/editor/main.tsx          React kökü, sorgu parametreleri
apps/desktop/src-tauri/src/editor.rs      pencere oluşturma
apps/desktop/src-tauri/src/commands.rs    export komutları (mevcut dosyaya eklenir)
```

---

### Task 1: Katman modeli ve komut yığını

**Files:** `packages/editor/{package.json,tsconfig.json}`, `src/{model.ts,commands.ts,index.ts}`, `src/{model,commands}.test.ts`

**Interfaces (birebir):**

```ts
export type Point = { x: number; y: number }
export type Rect = { x: number; y: number; width: number; height: number }
export type Color = string            // '#RRGGBB'
export type StrokeStyle = { color: Color; width: number }
export type ShapeStyle = { stroke: StrokeStyle; fill: Color | null }
export type TextStyle = { color: Color; size: number; family: string }
export type BadgeStyle = { fill: Color; color: Color; size: number }

export type Layer =
  | { id: string; kind: 'arrow'; from: Point; to: Point; style: StrokeStyle }
  | { id: string; kind: 'rect' | 'ellipse'; rect: Rect; style: ShapeStyle }
  | { id: string; kind: 'line'; points: Point[]; style: StrokeStyle }
  | { id: string; kind: 'text'; rect: Rect; content: string; style: TextStyle }
  | { id: string; kind: 'highlight'; rect: Rect; color: Color }
  | { id: string; kind: 'obscure'; rect: Rect; mode: 'blur' | 'pixelate' | 'blackout'; intensity: number }
  | { id: string; kind: 'step'; center: Point; index: number; style: BadgeStyle }

export type EditorDocument = {
  width: number          // kaynak görüntü, piksel
  height: number
  crop: Rect | null      // null = kırpma yok
  layers: Layer[]
}

export function createDocument(width: number, height: number): EditorDocument
export function nextStepIndex(doc: EditorDocument): number   // mevcut step katmanlarının en büyüğü + 1, yoksa 1
export function boundsOf(layer: Layer): Rect                 // her katman türü için sınırlayıcı kutu

export type Command = { apply(doc: EditorDocument): EditorDocument; invert(doc: EditorDocument): Command }
export function addLayer(layer: Layer): Command
export function removeLayer(id: string): Command
export function updateLayer(id: string, next: Layer): Command
export function setCrop(crop: Rect | null): Command

export class History {
  constructor(initial: EditorDocument)
  get document(): EditorDocument
  get canUndo(): boolean
  get canRedo(): boolean
  run(command: Command): void
  undo(): void
  redo(): void
}
```

- [ ] **Step 1: Testleri yaz (bunlar sözleşmedir, birebir)**

`model.test.ts`:
- `nextStepIndex` boş belgede 1 döner.
- `nextStepIndex`, index'leri 1 ve 3 olan iki step varken 4 döner (en büyük + 1, sayı + 1 değil).
- `boundsOf` bir ok için, uçları hangi sırada verilirse verilsin aynı pozitif kutuyu döner.
- `boundsOf` bir `line` için tüm noktaları kapsar.
- `createDocument` `crop: null` ve boş katman listesiyle döner.

`commands.test.ts`:
- `addLayer` sonrası belge katmanı içerir, `invert().apply()` sonrası içermez.
- `updateLayer`'ın tersi, katmanı **eski** haline döndürür (yeni haline değil).
- `removeLayer`'ın tersi katmanı **aynı sıradaki** yerine geri koyar, sona değil.
- `History.undo` üç komuttan sonra sırayla geri alır, `redo` aynı sırayla yeniden uygular.
- `run` çağrısı redo yığınını temizler (undo, undo, yeni komut, redo mümkün değil).
- `History` belgeyi mutasyona uğratmaz: `run` öncesi alınan referans değişmez.

- [ ] **Step 2: Testlerin başarısız olduğunu gör, sonra uygula.**
- [ ] **Step 3: `pnpm test`, `pnpm lint` temiz.**
- [ ] **Step 4: Commit** `feat(editor): add layer model and undo/redo command stack`

---

### Task 2: Seçim ve taşıma geometrisi

**Files:** `packages/editor/src/hit.ts`, `src/hit.test.ts`

**Interfaces (birebir):**

```ts
export type Handle = 'nw' | 'n' | 'ne' | 'e' | 'se' | 's' | 'sw' | 'w'
export function layerAtPoint(layers: Layer[], point: Point, tolerance: number): Layer | null
export function handleAtPoint(layer: Layer, point: Point, handleSize: number): Handle | null
export function moveLayer(layer: Layer, dx: number, dy: number): Layer
export function resizeLayer(layer: Layer, handle: Handle, pointer: Point, origin: Layer): Layer
```

**Interfaces (consumed):** Task 1'in `Layer`, `Point`, `Rect`, `boundsOf`.

- [ ] **Step 1: Testleri yaz**
- `layerAtPoint` üst üste binen iki katmanda **sonuncuyu** (en üstteki) döner.
- `layerAtPoint` bir okun gövdesine `tolerance` içinde tıklandığında onu bulur, uzağında `null` döner.
- `layerAtPoint` dolgusuz bir dikdörtgenin **içine** tıklandığında onu bulmaz, kenarına tıklandığında bulur.
- `handleAtPoint` sekiz tutamağın her birini kendi konumunda bulur, ortada `null` döner.
- `moveLayer` her katman türünü kaydırır ve türü korur (`line`'ın tüm noktaları, `step`'in merkezi).
- `resizeLayer` sabit `origin`'den hesaplar: karşı kenarı geçen iki adımlık bir sürükleme, kayış değil büyüme üretir. (Bu, overlay'de aynı hatanın yakalandığı testin editör karşılığıdır.)

- [ ] **Step 2-4:** RED, uygula, `pnpm test` + `pnpm lint`, commit `feat(editor): add selection and transform geometry`

---

### Task 3: Render ve gizleme düzleştirmesi

**Files:** `packages/editor/src/render.ts`, `src/export.ts`, `src/{render,export}.test.ts`

**Interfaces (birebir):**

```ts
export type RenderTarget = CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D
export function renderDocument(ctx: RenderTarget, image: CanvasImageSource, doc: EditorDocument): void
export function exportCanvas(image: CanvasImageSource, doc: EditorDocument): OffscreenCanvas
export async function toBlob(image: CanvasImageSource, doc: EditorDocument, type: 'image/png' | 'image/jpeg' | 'image/webp', quality?: number): Promise<Blob>
```

**Kurallar:**
- `renderDocument` ekranda ve dışa aktarmada aynı koddur. Ölçek, çağıranın verdiği transform ile ayarlanır; render fonksiyonu ekran ölçeği bilmez.
- Katmanlar dizideki sırayla çizilir, sonuncu en üstte.
- `obscure` katmanı **kaynak pikselleri okuyup** bulanıklaştırılmış veya pikselleştirilmiş sonucu yazar; `blackout` düz dolgudur. Dışa aktarılan tuvalde orijinal pikseller kalmaz.
- `crop` verildiğinde çıktı tuvali kırpılmış boyuttadır ve katmanlar kırpma başlangıcına göre kaydırılır.

- [ ] **Step 1: Testleri yaz** (Vitest, `OffscreenCanvas` yoksa `node-canvas` yerine jsdom + `canvas` paketi kullanma; testler `OffscreenCanvas` destekleyen ortamda koşar, yoksa Playwright ile sürülür ve bu durum raporda yazılır)
- Boş belge: çıktı kaynak görüntüyle piksel piksel aynıdır.
- `blackout` bölgesi: çıktıda o dikdörtgenin **her** pikseli düz dolgu rengidir ve kaynak değerlerden hiçbiri kalmamıştır.
- `pixelate`: bölge içindeki farklı piksel sayısı, blok sayısını aşmaz.
- `blur`: bölge içindeki komşu piksel farkı kaynağınkinden küçüktür (yumuşama ölçülür, göz kararı değil).
- `crop`: çıktı boyutu kırpma boyutudur ve kırpma dışındaki bir katman görünmez.
- Katman sırası: sonra eklenen üstte çizilir.

- [ ] **Step 2-4:** RED, uygula, testler, commit `feat(editor): add pure renderer and privacy-safe export`

---

### Task 4: Editör arayüzü

**Files:** `packages/editor/src/Editor.tsx`, `src/index.ts` (dışa aktarım)

**Interfaces (birebir):**

```ts
export type EditorProps = {
  image: CanvasImageSource
  width: number
  height: number
  onExport(blob: Blob, type: string): void | Promise<void>
  onCopy(blob: Blob): void | Promise<void>
  onClose(): void
}
export function Editor(props: EditorProps): JSX.Element
```

**Kurallar:**
- Araçlar: seç, ok, kutu, elips, serbest çizgi, metin, vurgu, gizle (blur/pixelate/blackout), adım numarası, kırp.
- Klavye: `Cmd+Z` geri al, `Cmd+Shift+Z` yinele, `Delete` seçili katmanı sil, `Cmd+C` panoya kopyala, `Cmd+S` kaydet, `Esc` seçimi bırak, tekrar `Esc` pencereyi kapat.
- Renk ve kalınlık seçimi araç çubuğunda; seçili katman varken değiştirmek o katmanı günceller (tek komut, geri alınabilir).
- Ekranda çizim `renderDocument` ile yapılır; ikinci bir çizim yolu yoktur.
- Bileşen Tauri bilmez: kaydetme ve kopyalama `props` üzerinden dışarı verilir.

- [ ] **Step 1:** Playwright ile gerçek bileşen üzerinde sürülerek doğrulanır (Task 7-9'da kurulan yöntem): her araç bir katman üretir, `Cmd+Z` geri alır, seçili katmanın rengi değişir, kırpma çıktı boyutunu değiştirir.
- [ ] **Step 2-3:** uygula, `pnpm lint` + `pnpm test` + `pnpm build`, commit `feat(editor): add the editing interface`

---

### Task 5: Masaüstü penceresi ve dışa aktarma

**Files:** `apps/desktop/editor.html`, `apps/desktop/src/editor/main.tsx`, `apps/desktop/src-tauri/src/editor.rs`, `commands.rs` (ekleme), `vite.config.ts` (giriş), `lib.rs` (modül)

**Kurallar:**
- Yakalama akışı **değişmez**: dosya yazılır, panoya kopyalanır. Ardından editör penceresi açılır ve o dosyayı gösterir.
- Editörden kaydetmek **aynı dosyayı** günceller; farklı format seçilirse yanına yeni dosya yazılır.
- Editör penceresi normal bir pencere: başlık çubuğu var, yeniden boyutlanır, ekranın ortasında açılır, görüntüden büyük olamaz.
- Görüntü asset protokolüyle yüklenir; IPC üzerinden piksel taşınmaz. Overlay'de kanıtlandığı gibi `crossOrigin = 'anonymous'` gerekir, çünkü `getImageData` okunacaktır.
- Yeni komutlar `commands.rs` içine eklenir, mevcut `close_overlays`, `list_windows`, `capture_region`, izin komutları **değiştirilmez**.

```rust
#[tauri::command] pub async fn save_edited(app: AppHandle, path: String, bytes: Vec<u8>) -> Result<String, String>
#[tauri::command] pub async fn copy_edited(app: AppHandle, bytes: Vec<u8>) -> Result<(), String>
#[tauri::command] pub fn close_editor(app: AppHandle)
```

- [ ] **Step 1:** `save_edited` için birim testi: var olan dosyanın üzerine yazar, farklı uzantı verilirse yeni dosya üretir, dizin dışına yazma girişimi reddedilir.
- [ ] **Step 2-4:** uygula, tüm kapılar, commit `feat(app): open the editor after a capture`

---

### Task 6: Uçtan uca doğrulama ve belgeler

- [ ] Gerçek girdiyle, paketli `.app` ile: yakala, editörde ok ve gizleme ekle, kaydet; dosyanın gizlenen bölgesinde orijinal piksellerin kalmadığını **ölçerek** doğrula.
- [ ] `Cmd+Z`, `Cmd+C`, `Esc` gerçek tuş vuruşlarıyla doğrulanır (`key code`, `keystroke` değil: Türkçe klavyede karakter eşlemesi farklı).
- [ ] Ölçüm release paketiyle yapılır; debug derlemesinde zamanlamalar yanıltıcıdır.
- [ ] README güncellenir: editör bölümü, kısayollar, gizleme davranışı.
- [ ] Commit `docs: document the editor`

