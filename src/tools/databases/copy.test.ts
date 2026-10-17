import { describe, expect, it } from 'vitest'
import { cellText, toCsv, toJson, toMarkdown, toTsv } from './copy'

const cols = ['id', 'note']

describe('cellText', () => {
  it('shows null as NULL and keeps the empty string', () => {
    expect(cellText(null)).toBe('NULL')
    expect(cellText('')).toBe('')
    expect(cellText('NULL')).toBe('NULL')
  })
})

describe('toTsv', () => {
  it('joins with tabs and newlines, with a header by default', () => {
    expect(toTsv(cols, [['1', 'a']])).toBe('id\tnote\n1\ta')
    expect(toTsv(cols, [['1', 'a']], false)).toBe('1\ta')
  })
  it('replaces tab, newline and carriage return by a space; null is an empty cell', () => {
    expect(toTsv(cols, [['1', 'a\tb\nc\rd'], ['2', null]], false)).toBe('1\ta b c d\n2\t')
  })
  it('keeps the empty string and null both empty', () => {
    expect(toTsv(cols, [['', null]], false)).toBe('\t')
  })
})

describe('toCsv', () => {
  it('quotes values with comma, quote or line break and doubles inner quotes', () => {
    expect(toCsv(cols, [['1', 'a,b'], ['2', 'say "hi"'], ['3', 'x\ny'], ['4', 'x\ry']])).toBe(
      'id,note\r\n1,"a,b"\r\n2,"say ""hi"""\r\n3,"x\ny"\r\n4,"x\ry"',
    )
  })
  it('leaves null empty and unquoted, and the empty string empty', () => {
    expect(toCsv(cols, [['1', null], ['2', '']])).toBe('id,note\r\n1,\r\n2,')
  })
  it('does not quote a tab or a pipe', () => {
    expect(toCsv(cols, [['1', 'a\tb|c']])).toBe('id,note\r\n1,a\tb|c')
  })
})

describe('toJson', () => {
  it('produces an array of objects with 2-space indent and null values', () => {
    expect(toJson(cols, [['1', null]])).toBe('[\n  {\n    "id": "1",\n    "note": null\n  }\n]')
    expect(JSON.parse(toJson(cols, [['1', '']]))).toEqual([{ id: '1', note: '' }])
  })
  it('suffixes duplicate column names with _2, _3', () => {
    expect(JSON.parse(toJson(['a', 'a', 'b', 'a'], [['1', '2', '3', '4']]))).toEqual([
      { a: '1', a_2: '2', b: '3', a_3: '4' },
    ])
  })
  it('returns an empty array without rows', () => {
    expect(toJson(cols, [])).toBe('[]')
  })
})

describe('toMarkdown', () => {
  it('writes a header and a separator row', () => {
    expect(toMarkdown(cols, [['1', 'a']])).toBe('| id | note |\n| --- | --- |\n| 1 | a |')
  })
  it('escapes pipes, turns newlines into <br> and writes null as NULL', () => {
    expect(toMarkdown(cols, [['1', 'a|b\nc'], ['2', null], ['3', '']])).toBe(
      '| id | note |\n| --- | --- |\n| 1 | a\\|b<br>c |\n| 2 | NULL |\n| 3 |  |',
    )
  })
})
