// The hello widget: one anchor call through the host bridge.
export async function mount(root, api) {
  const plugin = api.snapshot().plugin
  const heading = document.createElement('p')
  heading.textContent = `${plugin.name} ${plugin.version}`
  const line = document.createElement('p')
  line.textContent = 'calling /greeting…'
  root.replaceChildren(heading, line)
  api.resize(56)
  try {
    const reply = await api.call('/greeting', null, 'GET')
    line.textContent =
      reply.body && typeof reply.body === 'object' && reply.body.message
        ? reply.body.message
        : String(reply.body)
  } catch (error) {
    line.textContent = `refused: ${error.message ?? error}`
  }
  api.resize(64)
}
