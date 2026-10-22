import { LitElement, css, html, nothing } from 'lit'
import { customElement, property } from 'lit/decorators.js'

/**
 * Shared page shell for the dev-only harnesses.
 *
 * Slots:
 * - `description`: the paragraph below the heading.
 * - `actions`: controls rendered directly below the header.
 * - default: the harness body, in whatever order the page needs.
 *
 * The common light-DOM styles live in `harness.css`; this component owns the
 * header and the page width so every harness stays visually consistent.
 */
@customElement('anth-harness')
export class AnthHarness extends LitElement {
  @property()
  heading = ''

  @property()
  eyebrow = 'dev harness · not in the production build'

  @property()
  width = '1180px'

  override render() {
    return html`
      <main class="shell" style=${`--harness-width: ${this.width}`}>
        <header class="header">
          ${this.eyebrow
            ? html`<span class="eyebrow">${this.eyebrow}</span>`
            : nothing}
          <h1>${this.heading}</h1>
          <div class="description">
            <slot name="description"></slot>
          </div>
        </header>

        <slot name="actions"></slot>
        <slot></slot>
      </main>
    `
  }

  static styles = css`
    :host {
      display: block;
    }

    .shell {
      width: min(var(--harness-width, 1180px), calc(100% - 2rem));
      margin: 0 auto;
      padding: 4rem 0 5rem;
    }

    .header {
      margin-bottom: 1.75rem;
    }

    .eyebrow {
      display: inline-block;
      margin-bottom: 0.9rem;
      padding: 0.2rem 0.55rem;
      color: var(--accent, #7dd3fc);
      font: 600 11px/1.4 var(--mono, monospace);
      letter-spacing: 0.08em;
      text-transform: uppercase;
      border: 1px solid rgba(125, 211, 252, 0.35);
      border-radius: 999px;
      background: rgba(125, 211, 252, 0.08);
    }

    h1 {
      margin: 0 0 0.5rem;
      font: 600 clamp(1.8rem, 4vw, 2.6rem) / 1.1 var(--mono, monospace);
      letter-spacing: -0.04em;
      color: var(--text, #dbe6f2);
    }

    .description {
      max-width: 72ch;
      color: var(--muted, #8b9bad);
    }

    slot[name='description']::slotted(*) {
      margin: 0;
      color: var(--muted, #8b9bad);
    }
    @media (max-width: 600px) {
      .shell {
        width: calc(100% - 1.5rem);
        padding: 2rem 0 3rem;
      }
    }
  `
}

declare global {
  interface HTMLElementTagNameMap {
    'anth-harness': AnthHarness
  }
}
