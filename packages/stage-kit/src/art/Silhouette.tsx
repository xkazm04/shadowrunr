// Placeholder for the future avatar: an original skull-sorcerer in bone and verdigris-bronze.
// Built from the bible (digest § 1): bone-white skull, verdigris-bronze high collar and ribcage,
// charcoal chancellor robes, a cracked obsidian crown of floating astrolabe shards, and the
// green-glass Phylactery Orb that pulses while he speaks. No hood, no ram staff, no purple.

export function Silhouette({ speaking, small = false }: { speaking: boolean; small?: boolean }) {
  return (
    <svg
      className={`silhouette${speaking ? " speaking" : ""}${small ? " small" : ""}`}
      viewBox="0 0 400 460"
      role="img"
      aria-label="Arch-Lich Malakor (placeholder silhouette)"
      preserveAspectRatio="xMidYMid meet"
    >
      <defs>
        <radialGradient id="sil-aura" cx="50%" cy="42%" r="55%">
          <stop offset="0%" stopColor="#2f8f78" stopOpacity="0.45" />
          <stop offset="60%" stopColor="#123a33" stopOpacity="0.18" />
          <stop offset="100%" stopColor="#000" stopOpacity="0" />
        </radialGradient>
        <linearGradient id="sil-bone" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stopColor="#f2ecdc" />
          <stop offset="70%" stopColor="#d9cfb6" />
          <stop offset="100%" stopColor="#a99d82" />
        </linearGradient>
        <linearGradient id="sil-bronze" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0%" stopColor="#6fc2a8" />
          <stop offset="55%" stopColor="#3b8a74" />
          <stop offset="100%" stopColor="#7a5a2e" />
        </linearGradient>
        <linearGradient id="sil-robe" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stopColor="#2a2e31" />
          <stop offset="100%" stopColor="#111315" />
        </linearGradient>
        <radialGradient id="sil-orb" cx="40%" cy="35%" r="65%">
          <stop offset="0%" stopColor="#d4ffef" />
          <stop offset="35%" stopColor="#57f0bd" />
          <stop offset="100%" stopColor="#0d5e48" />
        </radialGradient>
        <linearGradient id="sil-obsidian" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0%" stopColor="#3a3f48" />
          <stop offset="50%" stopColor="#0d0f13" />
          <stop offset="100%" stopColor="#252a33" />
        </linearGradient>
        <filter id="sil-glow" x="-50%" y="-50%" width="200%" height="200%">
          <feGaussianBlur stdDeviation="4" result="b" />
          <feMerge>
            <feMergeNode in="b" />
            <feMergeNode in="SourceGraphic" />
          </feMerge>
        </filter>
      </defs>

      <circle cx="200" cy="210" r="200" fill="url(#sil-aura)" />

      {/* Astrolabe ring and floating crown shards */}
      <g className="sil-crown">
        <ellipse className="sil-ring" cx="200" cy="96" rx="128" ry="24" fill="none" stroke="#b08a4a" strokeWidth="2" strokeDasharray="6 10" opacity="0.8" />
        <ellipse cx="200" cy="96" rx="104" ry="18" fill="none" stroke="#4fb69a" strokeWidth="1" opacity="0.5" />
        <polygon className="shard s1" points="128,92 140,52 150,96" fill="url(#sil-obsidian)" stroke="#5fd0b4" strokeWidth="1.2" />
        <polygon className="shard s2" points="158,70 172,22 184,76" fill="url(#sil-obsidian)" stroke="#5fd0b4" strokeWidth="1.2" />
        <polygon className="shard s3" points="190,62 200,6 212,64 200,72" fill="url(#sil-obsidian)" stroke="#8fffe0" strokeWidth="1.4" />
        <polygon className="shard s4" points="216,76 228,22 242,70" fill="url(#sil-obsidian)" stroke="#5fd0b4" strokeWidth="1.2" />
        <polygon className="shard s5" points="250,96 260,52 272,92" fill="url(#sil-obsidian)" stroke="#5fd0b4" strokeWidth="1.2" />
        <path d="M200 6 L204 34 L198 50" stroke="#9affe2" strokeWidth="1" fill="none" opacity="0.7" />
      </g>

      {/* Robes */}
      <path d="M40 460 L66 336 Q112 292 200 286 Q288 292 334 336 L360 460 Z" fill="url(#sil-robe)" stroke="#6a5330" strokeWidth="2" />
      <path d="M150 300 L176 460 M250 300 L224 460" stroke="#7a5a2e" strokeWidth="3" opacity="0.7" />

      {/* Ribcage opening in brass-verdigris */}
      <path d="M164 296 L200 400 L236 296 Z" fill="#090b0b" />
      <g fill="none" stroke="url(#sil-bronze)" strokeWidth="5" strokeLinecap="round">
        <path d="M176 314 Q200 326 224 314" />
        <path d="M181 334 Q200 346 219 334" />
        <path d="M187 354 Q200 364 213 354" />
        <path d="M200 300 L200 392" strokeWidth="4" />
      </g>

      {/* Phylactery Orb */}
      <g className="sil-orb" filter="url(#sil-glow)">
        <circle cx="200" cy="344" r="19" fill="url(#sil-orb)" />
        <path d="M190 336 L199 346 L206 333" stroke="#e8fff6" strokeWidth="1.2" fill="none" opacity="0.7" />
      </g>

      {/* High collar plates */}
      <path d="M124 304 L62 158 L104 186 L156 262 Z" fill="url(#sil-bronze)" stroke="#1f4a3f" strokeWidth="2" />
      <path d="M276 304 L338 158 L296 186 L244 262 Z" fill="url(#sil-bronze)" stroke="#1f4a3f" strokeWidth="2" />
      <path d="M90 196 L132 282 M310 196 L268 282" stroke="#a7e8d4" strokeWidth="1.2" opacity="0.6" />

      {/* Skull */}
      <path
        d="M142 168 C142 112 170 86 200 86 C230 86 258 112 258 168 C258 194 249 210 240 220 L236 244 C230 261 216 270 200 270 C184 270 170 261 164 244 L160 220 C151 210 142 194 142 168 Z"
        fill="url(#sil-bone)"
        stroke="#7c715a"
        strokeWidth="2"
      />
      <path d="M152 206 L170 214 L166 238 Z M248 206 L230 214 L234 238 Z" fill="#b5a98c" opacity="0.7" />
      <path d="M148 150 Q200 130 252 150" stroke="#b08a4a" strokeWidth="5" fill="none" />
      <polygon points="200,132 207,142 200,152 193,142" fill="url(#sil-obsidian)" stroke="#8fffe0" strokeWidth="1" />
      <path d="M160 164 L192 172 L187 194 L166 191 Z" fill="#0b0d0c" />
      <path d="M240 164 L208 172 L213 194 L234 191 Z" fill="#0b0d0c" />
      <g className="sil-eyes" filter="url(#sil-glow)">
        <circle cx="177" cy="181" r="5" fill="#6dffd2" />
        <circle cx="223" cy="181" r="5" fill="#6dffd2" />
      </g>
      <path d="M200 202 L191 222 L200 217 L209 222 Z" fill="#141716" />
      <path d="M176 238 L224 238 L220 252 L180 252 Z" fill="#efe8d6" stroke="#6d634e" strokeWidth="1.5" />
      <path d="M186 238 L186 252 M194 238 L194 252 M202 238 L202 252 M210 238 L210 252 M217 238 L217 251" stroke="#6d634e" strokeWidth="1.2" />
    </svg>
  );
}
