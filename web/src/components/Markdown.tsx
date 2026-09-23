import type { ReactNode } from 'react'

// A small, safe renderer for the handful of things replies actually use:
// paragraphs, lists, headings, fenced and inline code, bold and italic.
// It builds React elements, so nothing from the model is ever injected as HTML.

type Inline = { text: string; bold?: boolean; italic?: boolean; code?: boolean }

const INLINE = /(\*\*[^*]+\*\*|\*[^*]+\*|`[^`]+`)/g

function inlineRuns(line: string): Inline[] {
  return line
    .split(INLINE)
    .filter((part) => part !== '')
    .map((part) => {
      if (part.startsWith('**') && part.endsWith('**') && part.length > 4) {
        return { text: part.slice(2, -2), bold: true }
      }
      if (part.startsWith('`') && part.endsWith('`') && part.length > 2) {
        return { text: part.slice(1, -1), code: true }
      }
      if (part.startsWith('*') && part.endsWith('*') && part.length > 2) {
        return { text: part.slice(1, -1), italic: true }
      }
      return { text: part }
    })
}

function InlineRuns({ line }: { line: string }) {
  return (
    <>
      {inlineRuns(line).map((run, i) =>
        run.bold ? (
          <strong key={i}>{run.text}</strong>
        ) : run.italic ? (
          <em key={i}>{run.text}</em>
        ) : run.code ? (
          <code key={i}>{run.text}</code>
        ) : (
          <span key={i}>{run.text}</span>
        ),
      )}
    </>
  )
}

export function Markdown({ text }: { text: string }) {
  const blocks: ReactNode[] = []
  const lines = text.replace(/\r\n/g, '\n').split('\n')
  let i = 0
  let key = 0

  while (i < lines.length) {
    const line = lines[i]

    if (line.trim().startsWith('```')) {
      const language = line.trim().slice(3).trim()
      const body: string[] = []
      i += 1
      while (i < lines.length && !lines[i].trim().startsWith('```')) {
        body.push(lines[i])
        i += 1
      }
      i += 1
      blocks.push(
        <pre className="code-block" key={key++}>
          {language && <span className="code-language">{language}</span>}
          <code>{body.join('\n')}</code>
        </pre>,
      )
      continue
    }

    if (/^#{1,4}\s/.test(line)) {
      const level = line.match(/^#+/)![0].length
      const rest = line.replace(/^#+\s*/, '')
      blocks.push(
        <p className={`prose-heading h${Math.min(level, 4)}`} key={key++}>
          <InlineRuns line={rest} />
        </p>,
      )
      i += 1
      continue
    }

    if (/^\s*[-*]\s+/.test(line)) {
      const items: string[] = []
      while (i < lines.length && /^\s*[-*]\s+/.test(lines[i])) {
        items.push(lines[i].replace(/^\s*[-*]\s+/, ''))
        i += 1
      }
      blocks.push(
        <ul key={key++}>
          {items.map((item, j) => (
            <li key={j}>
              <InlineRuns line={item} />
            </li>
          ))}
        </ul>,
      )
      continue
    }

    if (/^\s*\d+\.\s+/.test(line)) {
      const items: string[] = []
      while (i < lines.length && /^\s*\d+\.\s+/.test(lines[i])) {
        items.push(lines[i].replace(/^\s*\d+\.\s+/, ''))
        i += 1
      }
      blocks.push(
        <ol key={key++}>
          {items.map((item, j) => (
            <li key={j}>
              <InlineRuns line={item} />
            </li>
          ))}
        </ol>,
      )
      continue
    }

    if (line.trim() === '') {
      i += 1
      continue
    }

    const paragraph: string[] = []
    while (i < lines.length && lines[i].trim() !== '' && !lines[i].trim().startsWith('```')) {
      paragraph.push(lines[i])
      i += 1
    }
    blocks.push(
      <p key={key++}>
        {paragraph.map((line, j) => (
          <span key={j}>
            {j > 0 && <br />}
            <InlineRuns line={line} />
          </span>
        ))}
      </p>,
    )
  }

  return <div className="markdown">{blocks}</div>
}
