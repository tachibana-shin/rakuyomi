const unavailable = (name) => () => {
  throw new Error(
    `RakuYomi: parse5 "${name}" is not bundled; cheerio parses with htmlparser2 ` +
      "(to use parse5, pass _useHtmlParser2: false to cheerio.load)",
  );
};
export const parse = unavailable("parse");
export const parseFragment = unavailable("parseFragment");
export const serializeOuter = unavailable("serializeOuter");
