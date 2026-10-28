// Read-only WireGuard widget: list interfaces, then show one.
export async function mount(root, api) {
  const list = document.createElement('div')
  const output = document.createElement('pre')
  output.style.whiteSpace = 'pre-wrap'
  output.style.margin = '0.4rem 0 0'
  root.replaceChildren(list, output)
  api.resize(90)
  const reply = await api.call('/interfaces', null, 'GET')
  const names = reply.body?.interfaces ?? []
  if (names.length === 0) {
    list.textContent = 'No WireGuard interfaces.'
    return
  }
  for (const name of names) {
    const button = document.createElement('button')
    button.type = 'button'
    button.textContent = name
    button.style.marginRight = '0.35rem'
    button.addEventListener('click', async () => {
      output.textContent = 'loading…'
      const shown = await api.call('/show', { interface: name }, 'POST')
      output.textContent = shown.body?.output ?? JSON.stringify(shown.body)
      api.resize(Math.max(120, output.scrollHeight + 60))
    })
    list.append(button)
  }
}
