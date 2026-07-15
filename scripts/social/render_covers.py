from __future__ import annotations

import os
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter, ImageFont, ImageOps


ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT / "assets" / "social" / "source"
OUTPUT = ROOT / "assets" / "social"
WIDGET = SOURCE / "weekly-collapsed-hires.png"


def font_path(*names: str) -> Path:
    font_dir = Path(os.environ["WINDIR"]) / "Fonts"
    for name in names:
        candidate = font_dir / name
        if candidate.exists():
            return candidate
    raise FileNotFoundError(f"None of these fonts is installed: {', '.join(names)}")


BOLD = font_path("msyhbd.ttc", "simhei.ttf", "SourceHanSansCN-Normal.ttf")
REGULAR = font_path("msyh.ttc", "SourceHanSansCN-Normal.ttf", "simhei.ttf")


def fit_background(path: Path, size: tuple[int, int]) -> Image.Image:
    with Image.open(path) as image:
        return ImageOps.fit(
            image.convert("RGB"),
            size,
            method=Image.Resampling.LANCZOS,
            centering=(0.5, 0.5),
        ).convert("RGBA")


def widget_layer(width: int) -> tuple[Image.Image, Image.Image]:
    with Image.open(WIDGET) as source:
        source = source.convert("RGBA")
        height = round(width * source.height / source.width)
        product = source.resize((width, height), Image.Resampling.LANCZOS)

        mask = source.getchannel("A").resize((width, height), Image.Resampling.LANCZOS)
        product.putalpha(mask)
        return product, mask


def add_widget(canvas: Image.Image, width: int, center: tuple[int, int]) -> None:
    product, mask = widget_layer(width)
    left = center[0] - product.width // 2
    top = center[1] - product.height // 2

    full_mask = Image.new("L", canvas.size, 0)
    full_mask.paste(mask, (left, top), mask)
    glow_mask = full_mask.filter(ImageFilter.GaussianBlur(55))

    cyan_glow = Image.new("RGBA", canvas.size, (52, 142, 255, 0))
    cyan_glow.putalpha(glow_mask.point(lambda value: round(value * 0.62)))
    canvas.alpha_composite(cyan_glow)

    violet_glow = Image.new("RGBA", canvas.size, (130, 82, 255, 0))
    violet_glow.putalpha(glow_mask.point(lambda value: round(value * 0.35)))
    canvas.alpha_composite(violet_glow, (35, 22))
    canvas.alpha_composite(product, (left, top))


def draw_centered_text(
    canvas: Image.Image,
    xy: tuple[int, int],
    text: str,
    font: ImageFont.FreeTypeFont,
    fill: tuple[int, int, int, int] = (247, 249, 255, 255),
    stroke_width: int = 0,
) -> None:
    draw = ImageDraw.Draw(canvas)
    shadow = Image.new("RGBA", canvas.size, (0, 0, 0, 0))
    shadow_draw = ImageDraw.Draw(shadow)
    shadow_draw.text(
        (xy[0], xy[1] + 8),
        text,
        font=font,
        anchor="mm",
        fill=(0, 0, 0, 175),
        stroke_width=stroke_width + 2,
        stroke_fill=(0, 0, 0, 120),
    )
    canvas.alpha_composite(shadow.filter(ImageFilter.GaussianBlur(8)))
    draw.text(
        xy,
        text,
        font=font,
        anchor="mm",
        fill=fill,
        stroke_width=stroke_width,
        stroke_fill=(11, 18, 51, 210),
    )


def draw_label(canvas: Image.Image, center: tuple[int, int], text: str, size: int) -> None:
    font = ImageFont.truetype(str(BOLD), size)
    draw = ImageDraw.Draw(canvas)
    box = draw.textbbox((0, 0), text, font=font)
    width = box[2] - box[0] + 78
    height = box[3] - box[1] + 42
    left = center[0] - width // 2
    top = center[1] - height // 2

    plate = Image.new("RGBA", canvas.size, (0, 0, 0, 0))
    plate_draw = ImageDraw.Draw(plate)
    plate_draw.rounded_rectangle(
        (left, top, left + width, top + height),
        radius=height // 2,
        fill=(12, 20, 55, 185),
        outline=(126, 151, 255, 165),
        width=2,
    )
    canvas.alpha_composite(plate)
    draw_centered_text(canvas, center, text, font, fill=(228, 235, 255, 255))


def add_accent(canvas: Image.Image, y: int) -> None:
    draw = ImageDraw.Draw(canvas)
    center = canvas.width // 2
    draw.rounded_rectangle((center - 55, y, center + 55, y + 8), radius=4, fill=(102, 151, 255, 255))
    draw.ellipse((center + 70, y - 2, center + 82, y + 10), fill=(153, 104, 255, 255))


def render_xiaohongshu() -> Path:
    canvas = fit_background(SOURCE / "xiaohongshu-background.png", (1242, 1656))
    add_accent(canvas, 118)
    draw_centered_text(canvas, (621, 245), "我做了个 Codex", ImageFont.truetype(str(BOLD), 82))
    draw_centered_text(canvas, (621, 355), "额度悬浮球", ImageFont.truetype(str(BOLD), 94))
    draw_centered_text(
        canvas,
        (621, 468),
        "周额度终于能一眼看懂了",
        ImageFont.truetype(str(REGULAR), 43),
        fill=(190, 210, 255, 255),
    )
    add_widget(canvas, 720, (621, 1000))
    draw_label(canvas, (621, 1515), "Windows · 免费开源", 39)
    path = OUTPUT / "xiaohongshu-cover.png"
    canvas.convert("RGB").save(path, quality=96, optimize=True)
    return path


def render_douyin() -> Path:
    canvas = fit_background(SOURCE / "douyin-background.png", (1080, 1920))
    add_accent(canvas, 275)
    draw_centered_text(canvas, (475, 390), "Codex 周额度", ImageFont.truetype(str(BOLD), 88))
    draw_centered_text(canvas, (475, 500), "悬浮球", ImageFont.truetype(str(BOLD), 100))
    add_widget(canvas, 700, (475, 1060))
    draw_label(canvas, (475, 1570), "Windows 免费开源", 40)
    path = OUTPUT / "douyin-cover.png"
    canvas.convert("RGB").save(path, quality=96, optimize=True)
    return path


if __name__ == "__main__":
    OUTPUT.mkdir(parents=True, exist_ok=True)
    for rendered in (render_xiaohongshu(), render_douyin()):
        with Image.open(rendered) as image:
            print(f"{rendered.relative_to(ROOT)}: {image.width}x{image.height}")
