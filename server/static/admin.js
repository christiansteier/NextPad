// Verwaltung Erscheinungsbild: Farbwähler und Textfeld gleichziehen, eigene Abstufungen ein- und ausblenden.
(() => {
  for (const pick of document.querySelectorAll('input[type=color][data-for]')) {
    const text = document.getElementById(pick.dataset.for)
    pick.addEventListener('input', () => { text.value = pick.value })
    text.addEventListener('input', () => { if (/^#[0-9a-fA-F]{6}$/.test(text.value)) pick.value = text.value.toLowerCase() })
  }
  const custom = document.getElementById('f-custom'), shades = document.getElementById('shades')
  const sync = () => { shades.hidden = !custom.checked }
  custom.addEventListener('change', sync); sync()
})()
