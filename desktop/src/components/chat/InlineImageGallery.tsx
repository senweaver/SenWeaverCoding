import { useEffect, useMemo, useRef, useState } from 'react'
import { ImageGalleryModal } from './ImageGalleryModal'
import { getBaseUrl } from '../../api/client'

const QUICK_SCAN_LIMIT_CHARS = 4000

function looksLikelyToContainImagePath(text: string): boolean {
  if (!text) return false
  const len = text.length
  const sample = len > QUICK_SCAN_LIMIT_CHARS ? text.slice(0, QUICK_SCAN_LIMIT_CHARS) : text
  return /\.(png|jpe?g|gif|webp|svg|bmp|avif|ico)/i.test(sample)
}

const IMAGE_EXTENSIONS = /\.(png|jpe?g|gif|webp|svg|bmp|avif|ico)$/i

export function extractImagePaths(text: string): string[] {

  const regex = /(?:^|[\s`"'(])(\/?(?:[A-Za-z]:[\\/]|\/)[^\s`"')<>]+\.(?:png|jpe?g|gif|webp|svg|bmp|avif|ico))/gim
  const paths: string[] = []
  const seen = new Set<string>()

  let match: RegExpExecArray | null
  while ((match = regex.exec(text)) !== null) {
    const p = match[1]!.trim()
    if (!seen.has(p) && IMAGE_EXTENSIONS.test(p)) {
      seen.add(p)
      paths.push(p)
    }
  }

  return paths
}

function fileUrl(filePath: string): string {
  return `${getBaseUrl()}/api/filesystem/file?path=${encodeURIComponent(filePath)}`
}

function fileName(filePath: string): string {
  return filePath.split('/').pop() || filePath
}

type Props = {
  text: string
}

export function InlineImageGallery({ text }: Props) {
  const [activeIndex, setActiveIndex] = useState<number | null>(null)
  const sentinelRef = useRef<HTMLDivElement | null>(null)
  const mayContainImages = useMemo(() => looksLikelyToContainImagePath(text), [text])
  const [shouldScan, setShouldScan] = useState<boolean>(() => {
    if (typeof IntersectionObserver === 'undefined') return mayContainImages
    return false
  })

  useEffect(() => {
    if (shouldScan) return
    if (!mayContainImages) return
    if (typeof IntersectionObserver === 'undefined') {
      setShouldScan(true)
      return
    }
    const node = sentinelRef.current
    if (!node) return
    const io = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          if (entry.isIntersecting) {
            setShouldScan(true)
            io.disconnect()
            break
          }
        }
      },
      { rootMargin: '120px 0px' },
    )
    io.observe(node)
    return () => io.disconnect()
  }, [mayContainImages, shouldScan])

  const imagePaths = useMemo(
    () => (shouldScan && mayContainImages ? extractImagePaths(text) : []),
    [shouldScan, mayContainImages, text],
  )

  const images = useMemo(
    () => imagePaths.map((p) => ({ src: fileUrl(p), name: fileName(p) })),
    [imagePaths],
  )

  if (!mayContainImages) return null
  if (!shouldScan) {
    return <div ref={sentinelRef} aria-hidden="true" style={{ width: 1, height: 1 }} />
  }
  if (images.length === 0) return null

  return (
    <>
      <div className="mt-3 space-y-2">
        <div className="flex items-center gap-1.5 text-[10px] font-semibold uppercase tracking-wider text-[var(--color-outline)]">
          <span className="material-symbols-outlined text-[12px]">image</span>
          {images.length === 1 ? '1 image' : `${images.length} images`}
        </div>
        <div className={`grid gap-2 ${images.length === 1 ? 'grid-cols-1' : 'grid-cols-2'}`}>
          {images.map((img, i) => (
            <button
              key={img.src}
              type="button"
              onClick={() => setActiveIndex(i)}
              className="group relative overflow-hidden rounded-xl border border-[var(--color-border)] bg-[var(--color-surface-container-low)] text-left shadow-sm transition-all hover:shadow-md hover:border-[var(--color-brand)]/40"
            >
              <img
                src={img.src}
                alt={img.name}
                loading="lazy"
                className="w-full object-cover"
                style={{ maxHeight: images.length === 1 ? 400 : 240 }}
                onError={(e) => {

                  (e.target as HTMLImageElement).closest('button')!.style.display = 'none'
                }}
              />
              <div className="absolute inset-0 flex items-center justify-center bg-black/0 opacity-0 transition-all group-hover:bg-black/20 group-hover:opacity-100">
                <span className="material-symbols-outlined rounded-full bg-white/90 p-2 text-[20px] text-[var(--color-text-primary)] shadow-lg">
                  fullscreen
                </span>
              </div>
              <div className="absolute bottom-0 left-0 right-0 bg-gradient-to-t from-black/60 to-transparent px-2.5 pb-2 pt-6">
                <span className="text-[10px] font-medium text-white/90 drop-shadow-sm">
                  {img.name}
                </span>
              </div>
            </button>
          ))}
        </div>
      </div>

      {activeIndex !== null && activeIndex >= 0 && (
        <ImageGalleryModal
          open={activeIndex !== null}
          images={images}
          activeIndex={activeIndex}
          onClose={() => setActiveIndex(null)}
          onSelect={setActiveIndex}
        />
      )}
    </>
  )
}
