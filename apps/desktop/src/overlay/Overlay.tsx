import { convertFileSrc } from '@tauri-apps/api/core'
import { appCacheDir, join } from '@tauri-apps/api/path'
import { useEffect, useState } from 'react'

export function Overlay({ displayId }: { displayId: number; mode: string; scale: number }) {
  const [src, setSrc] = useState<string | null>(null)

  useEffect(() => {
    void (async () => {
      const path = await join(await appCacheDir(), `frozen-${displayId}.png`)
      setSrc(convertFileSrc(path))
    })()
  }, [displayId])

  if (!src) return null
  return <img src={src} alt="" style={{ width: '100%', height: '100%', display: 'block' }} />
}
