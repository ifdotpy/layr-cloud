import type { Locator } from '@playwright/test';

export interface Contrast { foreground: string; background: string; foregroundRgb: string; backgroundRgb: string; ratio: number }

/** Canvas resolves the browser's actual CSS colors into sRGB bytes. */
export async function measureContrast(locator: Locator): Promise<Contrast> {
  return await locator.evaluate(element => {
    const canvas = document.createElement('canvas');
    canvas.width = canvas.height = 1;
    const context = canvas.getContext('2d', { colorSpace: 'srgb', willReadFrequently: true })!;
    const pixel = (colors: string[]) => {
      context.clearRect(0, 0, 1, 1);
      context.fillStyle = '#ffffff';
      context.fillRect(0, 0, 1, 1);
      for (const color of colors) {
        context.fillStyle = color;
        context.fillRect(0, 0, 1, 1);
      }
      const bytes = [...context.getImageData(0, 0, 1, 1).data];
      return bytes.slice(0, 3);
    };
    const luminance = (rgb: number[]) => rgb.map(value => {
      value /= 255;
      return value <= 0.04045 ? value / 12.92 : Math.pow((value + 0.055) / 1.055, 2.4);
    }).reduce((sum, value, index) => sum + value * [0.2126, 0.7152, 0.0722][index], 0);
    const hex = (rgb: number[]) => '#' + rgb.map(value => value.toString(16).padStart(2, '0')).join('');
    const style = getComputedStyle(element);
    const layers: string[] = [];
    for (let ancestor: Element | null = element; ancestor; ancestor = ancestor.parentElement) {
      const layer = getComputedStyle(ancestor);
      if (layer.backgroundImage !== 'none') throw new Error('Contrast measurement does not support background images.');
      layers.unshift(layer.backgroundColor);
    }
    const foreground = pixel([...layers, style.color]), background = pixel(layers);
    const light = luminance(foreground), dark = luminance(background);
    return {
      foreground: style.color,
      background: style.backgroundColor,
      foregroundRgb: hex(foreground),
      backgroundRgb: hex(background),
      ratio: (Math.max(light, dark) + 0.05) / (Math.min(light, dark) + 0.05),
    };
  });
}
