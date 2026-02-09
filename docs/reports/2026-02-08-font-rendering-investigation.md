# Investigation: Font Rendering Regression (Tofu/Boxes)
**Date:** 2026-02-08
**Status:** In Progress
**Dri:** @wycats, @copilot

## 1. Problem Statement
Users report that after a recent update, Unicode characters are rendering as "Tofu" (boxes) in Chromium-based applications (Microsoft Edge, VS Code).
- **Scope:** Color Emojis (e.g., 😀) AND Text Faces/Kaomoji (e.g., ಠ_ಠ).
- **Affected Apps:** Microsoft Edge, VS Code (RPM/Layered versions).
- **Environment:** Bazzite (Fedora 43 Rawhide base).

## 2. Timeline & Actions
- **T-0:** User reports broken emojis.
- **T-1 (The "Nuclear" Fix):** Hypothesis was that Fedora's `Noto-COLRv1` format was incompatible. We removed `google-noto-color-emoji-fonts` and forced `Twemoji` via strict fontconfig.
- **T-2 (The Regression):** User reports *all* emojis and specialized text (Kannada/Kaomoji) are now broken (boxes).
- **T-3 (The Diagnosis):** `fc-match` revealed that the strict fontconfig rule for Twemoji was capturing requests for text fonts (like Kannada for `ಠ`), but Twemoji lacks those glyphs.
- **T-4 (The Revert):** We reverted the font removal and the strict fontconfig.

## 3. Hypotheses & Verification

| ID | Hypothesis | Status | Evidence/Notes |
|----|------------|--------|----------------|
| **H1** | **The "Square One" Loop**<br>We are reverting to a state where explicit font packages are missing from the image. | 🟡 **Active** | PR #100 reverts removals. Need to confirm Base Image contains `google-noto-color-emoji-fonts`. |
| **H2** | **The "Flatpak Phantom"**<br>Apps are Flatpaks ignoring our host-level config. | 🔴 **Disproved** | `manifests/flatpak-apps.json` does not list Edge/Code. They are RPMs. |
| **H3** | **The "Cache Zombie"**<br>Correct fonts are present, but stale `fc-cache` is persisting bad maps. | ⚪️ **Pending** | Requires post-reboot verification (`fc-cache -r -v`). |
| **H4** | **Binding Priority**<br>Twemoji was set to `binding="strong"`, overriding fallback search. | 🟢 **Confirmed** | `fc-match` proved Twemoji was being selected for Kannada characters it didn't support. |

## 4. Evidence Log
### Terminal Findings
- `rpm -qa`: Edge/Code are installed as RPMs.
- `fc-match -s :lang=kn`: Showed `Twemoji.ttf` as top priority (Root Cause of T-2 regression).
- `Containerfile`: (Pending Verification) Should show NO removal of emoji fonts.

## 5. Next Steps
1.  **Merge PR #100**: Restore standard Fedora font configuration.
2.  **Rebuild & Reboot**: Apply changes.
3.  **Verify**: Open `/tmp/test-unicode.html` in Edge.
    - Expectation: 
        - Color Emoji -> Noto Color Emoji (Color, Vector)
        - Kaomoji (ಠ_ಠ) -> Noto Sans Kannada (Text)

## 6. Plan B: The Surgical Substitution
**Scenario:** After revert, Kaomoji are fixed, but standard Emojis are monochrome or ugly (because Noto COLRv1 is indeed failing in Electron).

**Strategy:** "Reject, Don't Force"
Instead of forcing \`Twemoji\` via strong binding (which hijacks text), we simply tell Fontconfig to **ignore** \`Noto Color Emoji\`. This forces a natural fallback to the next available emoji font (Twemoji) without breaking text lookup chains.

**Draft Config (\`99-reject-noto-colr.conf\`):**
\`\`\`xml
<?xml version="1.0"?>
<!DOCTYPE fontconfig SYSTEM "urn:fontconfig:fonts.dtd">
<fontconfig>
  <!-- 
    Plan B: If Noto COLRv1 renders poorly, we "hide" it from the system.
    This allows fallback to find Twemoji naturally without breaking
    text fonts (like Kannada) via forced strong bindings.
  -->
  <selectfont>
    <rejectfont>
      <pattern>
        <patelt name="family">
          <string>Noto Color Emoji</string>
        </patelt>
      </pattern>
    </rejectfont>
  </selectfont>

  <!-- Optional: Weak hint to prefer Twemoji if generic 'emoji' is asked -->
  <match target="pattern">
     <test name="family"><string>emoji</string></test>
     <edit name="family" mode="prepend" binding="weak">
       <string>Twemoji</string>
     </edit>
  </match>
</fontconfig>
\`\`\`
