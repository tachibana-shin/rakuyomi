// `Document`/`Element` (eval/javascript/b_document.dart / b_element.dart):
// DOM access is delegated to the host (`js/bridge.rs`, backed by
// `dom_query`). Elements are opaque host-side keys; a miss registers a null
// slot and every access on it resolves to the empty string, exactly like the
// app. `hasAttr` keeps the app's quirk of passing `this.html` (the app
// always resolves it to `false`).

export class Element {
    key: string;

    constructor(key: string) {
        this.key = key;
    }

    getString(type: string): string {
        return sendMessage("get_element_string", JSON.stringify([type, this.key]));
    }

    get text(): string {
        return this.getString("text");
    }
    get outerHtml(): string {
        return this.getString("outerHtml");
    }
    get innerHtml(): string {
        return this.getString("innerHtml");
    }
    get className(): string {
        return this.getString("className");
    }
    get localName(): string {
        return this.getString("localName");
    }
    get namespaceUri(): string {
        return this.getString("namespaceUri");
    }
    get getSrc(): string {
        return this.getString("getSrc");
    }
    get getImg(): string {
        return this.getString("getImg");
    }
    get getHref(): string {
        return this.getString("getHref");
    }
    get getDataSrc(): string {
        return this.getString("getDataSrc");
    }

    getElementSibling(type: string): Element {
        const key = sendMessage("ele_element_sibling", JSON.stringify([type, this.key]));
        return new Element(key);
    }
    get previousElementSibling(): Element {
        return this.getElementSibling("previousElementSibling");
    }
    get nextElementSibling(): Element {
        return this.getElementSibling("nextElementSibling");
    }

    getElementsListBy(type: string, name?: string): Element[] {
        name = name || "";
        const elements: Element[] = [];
        JSON.parse(sendMessage("ele_get_elements_by", JSON.stringify([type, name, this.key]))).forEach(
            (key: string) => {
                elements.push(new Element(key));
            }
        );
        return elements;
    }
    get children(): Element[] {
        return this.getElementsListBy("children");
    }
    getElementsByTagName(name: string): Element[] {
        return this.getElementsListBy("getElementsByTagName", name);
    }
    getElementsByClassName(name: string): Element[] {
        return this.getElementsListBy("getElementsByClassName", name);
    }

    xpath(xpath: string): string[] {
        return JSON.parse(sendMessage("xpath", JSON.stringify([xpath, this.key]))) as string[];
    }
    xpathFirst(xpath: string): string {
        return sendMessage("xpathFirst", JSON.stringify([xpath, this.key]));
    }

    selectFirst(selector: string): Element {
        const key = sendMessage("ele_selectFirst", JSON.stringify([selector, this.key]));
        return new Element(key);
    }
    select(selector: string): Element[] {
        const elements: Element[] = [];
        JSON.parse(sendMessage("ele_select", JSON.stringify([selector, this.key]))).forEach(
            (key: string) => {
                elements.push(new Element(key));
            }
        );
        return elements;
    }

    attr(attr: string): string {
        return sendMessage("ele_attr", JSON.stringify([attr, this.key]));
    }
    hasAttr(attr: string): string {
        // The app sends `this.html` here (undefined on an Element, so it
        // serialises as `null`); the host always resolves it to `false`.
        return sendMessage("ele_has_attr", JSON.stringify([undefined, attr]));
    }
}

export class Document {
    html: string;

    constructor(html: string) {
        this.html = html;
    }

    getElement(type: string): Element {
        const key = sendMessage("get_doc_element", JSON.stringify([this.html, type]));
        return new Element(key);
    }
    get body(): Element {
        return this.getElement("body");
    }
    get documentElement(): Element {
        return this.getElement("documentElement");
    }
    get head(): Element {
        return this.getElement("head");
    }
    get parent(): Element {
        return this.getElement("parent");
    }

    getString(type: string): string {
        return sendMessage("get_doc_string", JSON.stringify([this.html, type]));
    }
    get text(): string {
        return this.getString("text");
    }
    get outerHtml(): string {
        return this.getString("outerHtml");
    }

    selectFirst(selector: string): Element {
        const key = sendMessage("doc_select_first", JSON.stringify([this.html, selector]));
        return new Element(key);
    }
    select(selector: string): Element[] {
        const elements: Element[] = [];
        JSON.parse(sendMessage("doc_select", JSON.stringify([this.html, selector]))).forEach(
            (key: string) => {
                elements.push(new Element(key));
            }
        );
        return elements;
    }

    xpathFirst(xpath: string): string {
        return sendMessage("doc_xpath_first", JSON.stringify([this.html, xpath]));
    }
    xpath(xpath: string): string[] {
        return JSON.parse(sendMessage("doc_xpath", JSON.stringify([this.html, xpath]))) as string[];
    }

    getElementsListBy(type: string, name?: string): Element[] {
        name = name || "";
        const elements: Element[] = [];
        JSON.parse(sendMessage("doc_get_elements_by", JSON.stringify([this.html, type, name]))).forEach(
            (key: string) => {
                elements.push(new Element(key));
            }
        );
        return elements;
    }
    get children(): Element[] {
        return this.getElementsListBy("children");
    }
    getElementsByTagName(name: string): Element[] {
        return this.getElementsListBy("getElementsByTagName", name);
    }
    getElementsByClassName(name: string): Element[] {
        return this.getElementsListBy("getElementsByClassName", name);
    }
    getElementById(id: string): Element {
        const key = sendMessage("doc_get_element_by_id", JSON.stringify([this.html, id]));
        return new Element(key);
    }

    attr(attr: string): string {
        return sendMessage("doc_attr", JSON.stringify([this.html, attr]));
    }
    hasAttr(attr: string): string {
        return sendMessage("doc_has_attr", JSON.stringify([this.html, attr]));
    }
}
