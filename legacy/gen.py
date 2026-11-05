import asyncio, os, re, sys
import edge_tts

VOICE = "fr-FR-DeniseNeural"
d = sys.argv[1]
text = open(os.path.join(d, "input.txt"), encoding="utf-8").read()
text = re.sub(r"[*`#_]", "", text)
parts = [p.strip() for p in re.split(r"(?<=[.!?:])\s+|\r?\n+", text) if p.strip()]

chunks, cur = [], ""
for p in parts:
    limit = 90 if not chunks else 250
    if cur and len(cur) + len(p) > limit:
        chunks.append(cur)
        cur = p
    else:
        cur = (cur + " " + p).strip()
if cur:
    chunks.append(cur)

async def gen(i, t, sem):
    async with sem:
        tmp = os.path.join(d, f"{i}.tmp")
        try:
            await edge_tts.Communicate(t, VOICE, rate="+30%").save(tmp)
            os.replace(tmp, os.path.join(d, f"{i}.mp3"))
        except Exception as e:
            with open(os.path.join(d, "error.txt"), "a", encoding="utf-8") as f:
                f.write(str(e) + "\n")

async def main():
    with open(os.path.join(d, "count.txt"), "w") as f:
        f.write(str(len(chunks)))
    sem = asyncio.Semaphore(3)
    await asyncio.gather(*(gen(i, t, sem) for i, t in enumerate(chunks)))

asyncio.run(main())