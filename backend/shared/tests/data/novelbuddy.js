"use strict";
var __awaiter = (this && this.__awaiter) || function (thisArg, _arguments, P, generator) {
    function adopt(value) { return value instanceof P ? value : new P(function (resolve) { resolve(value); }); }
    return new (P || (P = Promise))(function (resolve, reject) {
        function fulfilled(value) { try { step(generator.next(value)); } catch (e) { reject(e); } }
        function rejected(value) { try { step(generator["throw"](value)); } catch (e) { reject(e); } }
        function step(result) { result.done ? resolve(result.value) : adopt(result.value).then(fulfilled, rejected); }
        step((generator = generator.apply(thisArg, _arguments || [])).next());
    });
};
var __generator = (this && this.__generator) || function (thisArg, body) {
    var _ = { label: 0, sent: function() { if (t[0] & 1) throw t[1]; return t[1]; }, trys: [], ops: [] }, f, y, t, g = Object.create((typeof Iterator === "function" ? Iterator : Object).prototype);
    return g.next = verb(0), g["throw"] = verb(1), g["return"] = verb(2), typeof Symbol === "function" && (g[Symbol.iterator] = function() { return this; }), g;
    function verb(n) { return function (v) { return step([n, v]); }; }
    function step(op) {
        if (f) throw new TypeError("Generator is already executing.");
        while (g && (g = 0, op[0] && (_ = 0)), _) try {
            if (f = 1, y && (t = op[0] & 2 ? y["return"] : op[0] ? y["throw"] || ((t = y["return"]) && t.call(y), 0) : y.next) && !(t = t.call(y, op[1])).done) return t;
            if (y = 0, t) op = [op[0] & 2, t.value];
            switch (op[0]) {
                case 0: case 1: t = op; break;
                case 4: _.label++; return { value: op[1], done: false };
                case 5: _.label++; y = op[1]; op = [0]; continue;
                case 7: op = _.ops.pop(); _.trys.pop(); continue;
                default:
                    if (!(t = _.trys, t = t.length > 0 && t[t.length - 1]) && (op[0] === 6 || op[0] === 2)) { _ = 0; continue; }
                    if (op[0] === 3 && (!t || (op[1] > t[0] && op[1] < t[3]))) { _.label = op[1]; break; }
                    if (op[0] === 6 && _.label < t[1]) { _.label = t[1]; t = op; break; }
                    if (t && _.label < t[2]) { _.label = t[2]; _.ops.push(op); break; }
                    if (t[2]) _.ops.pop();
                    _.trys.pop(); continue;
            }
            op = body.call(thisArg, _);
        } catch (e) { op = [6, e]; y = 0; } finally { f = t = 0; }
        if (op[0] & 5) throw op[1]; return { value: op[0] ? op[1] : void 0, done: true };
    }
};
Object.defineProperty(exports, "__esModule", { value: true });
var cheerio_1 = require("cheerio");
var fetch_1 = require("@libs/fetch");
var filterInputs_1 = require("@libs/filterInputs");
var novelStatus_1 = require("@libs/novelStatus");
var fwnRegex = /(?:𝐟|ᵮ|𝑓|𝒇|𝒻|𝓯|𝔣|𝕗|𝖿|𝗳|𝙛|𝚏|ꬵ|ꞙ|ẝ|𝖋|ⓕ|ｆ|ḟ|ʃ|բ|ᶠ|⒡|ſ|ꊰ|ʄ|∱|ᶂ|𝘧|\bf)(?:𝚛|ꭇ|ᣴ|ℾ|𝚪|𝛤|𝜞|𝝘|𝞒|Ⲅ|Г|Ꮁ|ᒥ|ꭈ|ⲅ|ꮁ|ⓡ|ｒ|ŕ|ṙ|ř|ȑ|ȓ|ṛ|ṝ|ŗ|г|Ր|ɾ|ᥬ|ṟ|ɍ|ʳ|⒭|ɼ|ѓ|ᴦ|ᶉ|𝐫|𝑟|𝒓|𝓇|𝓻|𝔯|𝕣|𝖗|𝗋|𝗿|𝘳|𝙧|ᵲ|ґ|ᵣ|r)(?:ə|ә|ⅇ|ꬲ|ꞓ|⋴|𝛆|𝛜|𝜀|𝜖|𝜺|𝝐|𝝴|𝞊|𝞮|𝟄|ⲉ|ꮛ|𐐩|Ꞓ|Ⲉ|⍷|𝑒|𝓮|𝕖|𝖊|𝘦|𝗲|𝚎|𝙚|𝒆|𝔢|𝖾|𝐞|Ҿ|ҿ|ⓔ|ｅ|⒠|è|ᧉ|é|ᶒ|ê|ɘ|ἔ|ề|ế|ễ|૯|ǝ|є|ε|ē|ҽ|ɛ|ể|ẽ|ḕ|ḗ|ĕ|ė|ë|ẻ|ě|ȅ|ȇ|ẹ|ệ|ȩ|ɇ|ₑ|ę|ḝ|ḙ|ḛ|℮|е|ԑ|ѐ|ӗ|ᥱ|ё|ἐ|ἑ|ἒ|ἓ|ἕ|ℯ|e)+(?:𝐰|ꝡ|𝑤|𝒘|𝓌|𝔀|𝔴|𝕨|𝖜|𝗐|𝘄|𝘸|𝙬|𝚠|ա|ẁ|ꮃ|ẃ|ⓦ|⍵|ŵ|ẇ|ẅ|ẘ|ẉ|ⱳ|ὼ|ὠ|ὡ|ὢ|ὣ|ω|ὤ|ὥ|ὦ|ὧ|ῲ|ῳ|ῴ|ῶ|ῷ|Ⱳ|ѡ|ԝ|ᴡ|ώ|ᾠ|ᾡ|ᾡ|ᾢ|ᾣ|ᾤ|ᾥ|ᾦ|ɯ|𝝕|𝟉|𝞏|w)(?:ə|ә|ⅇ|ꬲ|ꞓ|⋴|𝛆|𝛜|𝜀|𝜖|𝜺|𝝐|𝝴|𝞊|𝞮|𝟄|ⲉ|ꮛ|𐐩|Ꞓ|Ⲉ|⍷|𝑒|𝓮|𝕖|𝖊|𝘦|𝗲|𝚎|𝙚|𝒆|𝔢|𝖾|𝐞|Ҿ|ҿ|ⓔ|ｅ|⒠|è|ᧉ|é|ᶒ|ê|ɘ|ἔ|ề|ế|ễ|૯|ǝ|є|ε|ē|ҽ|ɛ|ể|ẽ|ḕ|ḗ|ĕ|ė|ë|ẻ|ě|ȅ|ȇ|ẹ|ệ|ȩ|ɇ|ₑ|ę|ḝ|ḙ|ḛ|℮|е|ԑ|ѐ|ӗ|ᥱ|ё|ἐ|ἑ|ἒ|ἓ|ἕ|ℯ|e)(?:ꮟ|Ꮟ|𝐛|𝘣|𝒷|𝔟|𝓫|𝖇|𝖻|𝑏|𝙗|𝕓|𝒃|𝗯|𝚋|♭|ᑳ|ᒈ|ｂ|ᖚ|ᕹ|ᕺ|ⓑ|ḃ|ḅ|ҍ|ъ|ḇ|ƃ|ɓ|ƅ|ᖯ|Ƅ|Ь|ᑲ|þ|Ƃ|⒝|Ъ|ᶀ|ᑿ|ᒀ|ᒂ|ᒁ|ᑾ|ь|ƀ|Ҍ|Ѣ|ѣ|ᔎ |b)(?:ո|ռ|ח|𝒏|𝓷|𝙣|𝑛|𝖓|𝔫|𝗇|𝚗|𝗻|ᥒ|ⓝ|ή|ｎ|ǹ|ᴒ|ń|ñ|ᾗ|η|ṅ|ň|ṇ|ɲ|ņ|ṋ|ṉ|ղ|ຖ|Ռ|ƞ|ŋ|⒩|ภ|ก|ɳ|п|ŉ|л|ԉ|Ƞ|ἠ|ἡ|ῃ|դ|ᾐ|ᾑ|ᾒ|ᾓ|ᾔ|ᾕ|ᾖ|ῄ|ῆ|ῇ|ῂ|ἢ|ἣ|ἤ|ἥ|ἦ|ἧ|ὴ|ή|በ|ቡ|ቢ|ባ|ቤ|ብ|ቦ|ȵ|𝛈|𝜂|𝜼|𝝶|𝞰|𝕟|延|𝐧|𝔫|ᶇ|ᵰ|ᥥ|∩|n)(?:ం|ం|ം|ං|૦|௦|۵|ℴ|𝑜|𝒐|𝒐|ꬽ|𝝄|𝛔|𝜎|𝝈|𝞂|ჿ|𝚘|০|୦|ዐ|𝛐|𝗈|𝞼|ဝ|ⲟ|𝙤|၀|𐐬|𝔬|𐓪|𝓸|🇴|⍤|○|ϙ|🅾|𝒪|𝖮|𝟢|𝟶|𝙾|o|𝗼|𝕠|𝜊|𝐨|𝝾|𝞸|ᐤ|ｵ|ѳ|᧐|ᥲ|ð|ｏ|ఠ|ᦞ|Փ|ò|ө|ӧ|ó|º|ō|ô|ǒ|ȏ|ŏ|ồ|ȭ|ṏ|ὄ|ṑ|ṓ|ȯ|ȫ|๏|ᴏ|ő|ö|ѻ|о|ዐ|ǭ|ȱ|০|୦|٥|౦|告知|๐|໐|ο|օ|ᴑ|०|੦|ỏ|ơ|ờ|ớ|ỡ|ở|ợ|ọ|ộ|ǫ|ø|ǿ|ɵ|ծ|ὀ|ὁ|ό|ὸ|ό|ὂ|ὃ|ὅ|o)(?:∨|⌄|\|ⅴ|𝐯|𝑣|𝒗|𝓋|𝔳|𝕧|𝖛|ꮩ|ሀ|ⓥ|ｖ|𝜐|𝝊|ṽ|ṿ|౮|ง|ѵ|ע|ᴠ|ν|ט|ᵥ|ѷ|៴|ᘁ|𝙫|𝙫|𝛎|𝜈|𝝂|𝝼|𝞶|𝘷|𝘃|𝓿|v)(?:ə|ә|ⅇ|ꬲ|ꞓ|⋴|𝛆|𝛜|𝜀|𝜖|𝜺|𝝐|𝝴|𝞊|𝞮|𝟄|ⲉ|ꮛ|𐐩|Ꞓ|Ⲉ|⍷|𝑒|𝓮|𝕖|𝖊|𝘦|𝗲|𝚎|𝙚|𝒆|𝔢|𝖾|𝐞|Ҿ|ҿ|ⓔ|ｅ|⒠|è|ᧉ|é|ᶒ|ê|ɘ|ἔ|ề|ế|ễ|૯|ǝ|є|ε|ē|ҽ|ɛ|ể|ẽ|ḕ|ḗ|ĕ|ė|ë|ẻ|ě|ȅ|ȇ|ẹ|ệ|ȩ|ɇ|ę|ḝ|ḙ|ḛ|℮|е|ԑ|ѐ|ӗ|ᥱ|ё|ἐ|ἑ|ἒ|ἓ|ἕ|ℯ|e)(?:ⓛ|ｌ|ŀ|ĺ|ľ|ḷ|ḹ|ḷ|ļ|Ӏ|ℓ|ḽ|ḻ|ł|ﾚ|ɭ|ƚ|ɫ|ⱡ|\||\\|Ɩ|⒧|ʅ|ǀ|ו|ן|Ι|І|｜|ᶩ|ӏ|𝓘|𝕀|𝖨|𝗜|𝘐|𝐥|𝑙|𝒍|𝓁|𝔩|𝕝|𝖑|ލ|𝗅|𝗹|ލ|𝗅|𝗹|𝘭|𝚕|𝜤|𝝞|ı|𝚤|ɩ|ι|𝛊|𝜄|𝜾|𝞲|I|l)(?:.?(?:🝌|ｃ|ⅽ|𝐜|𝑐|𝒄|𝒸|𝓬|𝔠|𝕔|𝖈|𝖈|𝗰|𝘤|𝙘|𝚌|ᴄ|ϲ|ⲥ|с|ꮯ|𐐽|ⲥ|𐐽|ꮯ|ĉ|ｃ|ⓒ|ć|č|ċ|ç|ҁ|ƈ|ḉ|ȼ|ↄ|с|ር|ᴄ|ϲ|ҫ|꒝|ς|ɽ|ϛ|𝙲|ᑦ|᧚|𝐜|𝑐|𝒄|𝒸|𝓬|𝔠|𝕔|𝖈|𝖈|𝗰|𝘤|𝙘|𝚌|₵|🇨|ᥴ|ᒼ|ⅽ|c)(?:ం|ం|ം|ං|૦|௦|۵|ℴ|𝑜|𝒐|𝒐|ꬽ|𝝄|𝛔|𝜎|𝝈|𝞂|ჿ|𝚘|০|୦|ዐ|𝗈|𝞼|ဝ|ⲟ|𝙤|၀|𐐬|𝔬|𐓪|𝓸|🇴|⍤|○|ϙ|🅾|𝒪|𝖮|𝟢|𝟶|𝙾|o|𝗼|𝕠|𝜊|𝐨|𝝾|𝞸|ᐤ|ⓞ|ѳ|᧐|ᥲ|ð|ｏ|ఠ|ᦞ|Փ|ò|ө|ӧ|ó|º|ō|ô|ǒ|ȏ|ŏ|ồ|ȭ|ṏ|ὄ|ṑ|ṓ|ȯ|ȫ|๏|ᴏ|ő|ö|ѻ|о|ዐ|ǭ|ȱ|০|୦|٥|౦|告知|๐|໐|ο|օ|ᴑ|०|੦|ỏ|ơ|ờ|ớ|ỡ|ở|ợ|ọ|ộ|ǫ|ø|ǿ|ɵ|ծ|ὀ|ὁ|ό|ὸ|ό|ὂ|ὃ|ὅ|o)(?:₥|ᵯ|𝖒|𝐦|𝗆|𝔪|𝕞|𝕞|𝕞|ⓜ|ｍ|ന|ᙢ|൩|ḿ|ṁ|ⅿ|ϻ|ṃ|ጠ|ɱ|៳|ᶆ|𝒎|🇲|𝙢|𝓶|𝚖|𝑚|𝗺|᧕|᧗|m))?/g;
var NovelBuddy = /** @class */ (function () {
    function NovelBuddy() {
        this.id = 'novelbuddy';
        this.name = 'NovelBuddy';
        this.site = 'https://novelbuddy.me/';
        this.api = 'https://api.novelbuddy.me/';
        this.version = '2.1.3';
        this.icon = 'src/en/novelbuddy/icon.png';
        this.filters = {
            orderBy: {
                value: 'views',
                label: 'Order by',
                options: [
                    { label: 'Default Order', value: '' },
                    { label: 'Most Viewed', value: 'views' },
                    { label: 'Latest Updated', value: 'latest' },
                    { label: 'Most Popular', value: 'popular' },
                    { label: 'A-Z', value: 'alphabetical' },
                    { label: 'Highest Rating', value: 'rating' },
                    { label: 'Most Chapters', value: 'chapters' },
                ],
                type: filterInputs_1.FilterTypes.Picker,
            },
            keyword: { value: '', label: 'Keywords', type: filterInputs_1.FilterTypes.TextInput },
            status: {
                value: 'all',
                label: 'Status',
                options: [
                    { label: 'All', value: 'all' },
                    { label: 'Ongoing', value: 'ongoing' },
                    { label: 'Completed', value: 'completed' },
                    { label: 'Hiatus', value: 'hiatus' },
                    { label: 'Cancelled', value: 'cancelled' },
                ],
                type: filterInputs_1.FilterTypes.Picker,
            },
            genre: {
                value: { include: [], exclude: [] },
                label: 'Genres (OR, not AND)',
                options: [
                    { label: 'Action', value: 'action' },
                    { label: 'ActionAdventure', value: 'actionadventure' },
                    { label: 'Adult', value: 'adult' },
                    { label: 'Adventure', value: 'adventure' },
                    { label: 'Comedy', value: 'comedy' },
                    { label: 'Drama', value: 'drama' },
                    { label: 'Eastern', value: 'eastern' },
                    { label: 'Easterni', value: 'easterni' },
                    { label: 'Ecchi', value: 'ecchi' },
                    { label: 'Fan-Fiction', value: 'fan-fiction' },
                    { label: 'Fantasy', value: 'fantasy' },
                    { label: 'Game', value: 'game' },
                    { label: 'Games', value: 'games' },
                    { label: 'Gender Bender', value: 'gender-bender' },
                    { label: 'Harem', value: 'harem' },
                    { label: 'Historical', value: 'historical' },
                    { label: 'Horror', value: 'horror' },
                    { label: 'Isekai', value: 'isekai' },
                    { label: 'Josei', value: 'josei' },
                    { label: 'Lolicon', value: 'lolicon' },
                    { label: 'Magic', value: 'magic' },
                    { label: 'Martial Arts', value: 'martial-arts' },
                    { label: 'Mature', value: 'mature' },
                    { label: 'Mecha', value: 'mecha' },
                    { label: 'Military', value: 'military' },
                    { label: 'Modern Life', value: 'modern-life' },
                    { label: 'Movies', value: 'movies' },
                    { label: 'Mystery', value: 'mystery' },
                    { label: 'Psychologic', value: 'psychologic' },
                    { label: 'Psychological', value: 'psychological' },
                    { label: 'Reincarnatio', value: 'reincarnatio' },
                    { label: 'Reincarnation', value: 'reincarnation' },
                    { label: 'Romanc', value: 'romanc' },
                    { label: 'Romance', value: 'romance' },
                    { label: 'Romance.Adventure', value: 'romance-adventure' },
                    { label: 'RomanceAdventure', value: 'romanceadventure' },
                    { label: 'Romance.Harem', value: 'romance-harem' },
                    { label: 'RomanceHarem', value: 'romanceharem' },
                    { label: 'Romance.Smut', value: 'romance-smut' },
                    { label: 'Romancei', value: 'romancei' },
                    { label: 'Romancem', value: 'romancem' },
                    { label: 'School Life', value: 'school-life' },
                    { label: 'Sci-fi', value: 'sci-fi' },
                    { label: 'Seinen', value: 'seinen' },
                    { label: 'Seinen Wuxia', value: 'seinen-wuxia' },
                    { label: 'Shoujo', value: 'shoujo' },
                    { label: 'Shoujo Ai', value: 'shoujo-ai' },
                    { label: 'Shounen', value: 'shounen' },
                    { label: 'Shounen Ai', value: 'shounen-ai' },
                    { label: 'Slice of Lif', value: 'slice-of-lif' },
                    { label: 'Slice Of Life', value: 'slice-of-life' },
                    { label: 'Slice of Lifel', value: 'slice-of-lifel' },
                    { label: 'Smut', value: 'smut' },
                    { label: 'Sports', value: 'sports' },
                    { label: 'Superna', value: 'superna' },
                    { label: 'Supernatural', value: 'supernatural' },
                    { label: 'System', value: 'system' },
                    { label: 'Thriller', value: 'thriller' },
                    { label: 'Tragedy', value: 'tragedy' },
                    { label: 'Urban', value: 'urban' },
                    { label: 'Urban Life', value: 'urban-life' },
                    { label: 'Wuxia', value: 'wuxia' },
                    { label: 'Xianxia', value: 'xianxia' },
                    { label: 'Xuanhuan', value: 'xuanhuan' },
                    { label: 'Yaoi', value: 'yaoi' },
                    { label: 'Yuri', value: 'yuri' },
                ],
                type: filterInputs_1.FilterTypes.ExcludableCheckboxGroup,
            },
            min_ch: {
                value: '',
                label: 'Minimum Chapters',
                type: filterInputs_1.FilterTypes.TextInput,
            },
            max_ch: {
                label: 'Maximum Chapters',
                value: '',
                type: filterInputs_1.FilterTypes.TextInput,
            },
            type: {
                value: '',
                label: 'Types',
                options: [
                    { label: 'All Types', value: '' },
                    { label: 'Japanese comics', value: 'manga' },
                    { label: 'Korean comics', value: 'manhwa' },
                    { label: 'Chinese comics', value: 'manhua' },
                ],
                type: filterInputs_1.FilterTypes.Picker,
            },
            demo: {
                value: [],
                label: 'Demographics',
                options: [
                    { label: 'Shounen', value: 'shounen' },
                    { label: 'Shoujo', value: 'shoujo' },
                    { label: 'Seinen', value: 'seinen' },
                    { label: 'Josei', value: 'josei' },
                ],
                type: filterInputs_1.FilterTypes.CheckboxGroup,
            },
        };
    }
    NovelBuddy.prototype.parseNovels = function (body) {
        return body.data.items.map(function (item) { return ({
            name: item.name,
            path: item.url.startsWith('/') ? item.url.slice(1) : item.url,
            cover: item.cover,
        }); });
    };
    NovelBuddy.prototype.popularNovels = function (pageNo_1, _a) {
        return __awaiter(this, arguments, void 0, function (pageNo, _b) {
            var genre, min_ch, max_ch, status, demo, orderBy, keyword, parseNumber, rawParams, params, _i, _c, _d, key, value, url, result, body;
            var _e, _f, _g;
            var filters = _b.filters;
            return __generator(this, function (_h) {
                switch (_h.label) {
                    case 0:
                        genre = filters.genre, min_ch = filters.min_ch, max_ch = filters.max_ch, status = filters.status, demo = filters.demo, orderBy = filters.orderBy, keyword = filters.keyword;
                        parseNumber = function (val) {
                            if (!(val === null || val === void 0 ? void 0 : val.trim()))
                                return;
                            var n = Number(val);
                            return Number.isInteger(n) && n >= 0 && n <= 10000
                                ? String(n)
                                : undefined;
                        };
                        rawParams = {
                            genres: ((_e = genre.value.include) === null || _e === void 0 ? void 0 : _e.join(',')) || undefined,
                            exclude: ((_f = genre.value.exclude) === null || _f === void 0 ? void 0 : _f.join(',')) || undefined,
                            min_ch: parseNumber(min_ch.value),
                            max_ch: parseNumber(max_ch.value),
                            status: status.value !== 'all' ? String(status.value) : undefined,
                            demographic: ((_g = demo.value) === null || _g === void 0 ? void 0 : _g.join(',')) || undefined,
                            sort: String(orderBy.value),
                            page: String(pageNo),
                            limit: '24',
                            q: keyword.value || undefined,
                        };
                        params = new URLSearchParams();
                        for (_i = 0, _c = Object.entries(rawParams); _i < _c.length; _i++) {
                            _d = _c[_i], key = _d[0], value = _d[1];
                            if (value !== undefined)
                                params.append(key, value);
                        }
                        url = this.api + 'titles/search?' + params.toString();
                        return [4 /*yield*/, (0, fetch_1.fetchApi)(url)];
                    case 1:
                        result = _h.sent();
                        return [4 /*yield*/, result.json()];
                    case 2:
                        body = _h.sent();
                        return [2 /*return*/, this.parseNovels(body)];
                }
            });
        });
    };
    NovelBuddy.prototype.parseNovel = function (novelPath) {
        return __awaiter(this, void 0, void 0, function () {
            var response, body, scriptMatch, data, initialManga, novel, rawStatus, map, summaryStr, $, cv, chaptersUrl, chaptersResponse, chaptersJson;
            var _a, _b, _c, _d, _e;
            return __generator(this, function (_f) {
                switch (_f.label) {
                    case 0: return [4 /*yield*/, (0, fetch_1.fetchApi)(this.site + novelPath)];
                    case 1:
                        response = _f.sent();
                        return [4 /*yield*/, response.text()];
                    case 2:
                        body = _f.sent();
                        scriptMatch = body.match(/<script id="__NEXT_DATA__" type="application\/json">(.*?)<\/script>/);
                        if (!scriptMatch)
                            throw new Error('Could not find __NEXT_DATA__');
                        data = JSON.parse(scriptMatch[1]);
                        initialManga = data.props.pageProps.initialManga;
                        if (!initialManga)
                            throw new Error('Could not find initialManga data');
                        novel = {
                            path: novelPath,
                            name: initialManga.name || 'Untitled',
                            cover: initialManga.cover,
                            author: ((_a = initialManga.authors) === null || _a === void 0 ? void 0 : _a.map(function (a) { return a.name; }).join(', ')) || '',
                            artist: ((_b = initialManga.artists) === null || _b === void 0 ? void 0 : _b.map(function (a) { return a.name; }).join(', ')) || '',
                            genres: ((_c = initialManga.genres) === null || _c === void 0 ? void 0 : _c.map(function (g) { return g.name; }).join(',')) || '',
                            chapters: [],
                        };
                        rawStatus = initialManga.status;
                        map = {
                            ongoing: novelStatus_1.NovelStatus.Ongoing,
                            hiatus: novelStatus_1.NovelStatus.OnHiatus,
                            dropped: novelStatus_1.NovelStatus.Cancelled,
                            cancelled: novelStatus_1.NovelStatus.Cancelled,
                            completed: novelStatus_1.NovelStatus.Completed,
                        };
                        novel.status = (_d = map[rawStatus.toLowerCase()]) !== null && _d !== void 0 ? _d : novelStatus_1.NovelStatus.Unknown;
                        summaryStr = initialManga.summary || '';
                        if (summaryStr) {
                            $ = (0, cheerio_1.load)('<div>' + summaryStr + '</div>');
                            $('br').replaceWith('\n');
                            $('p').before('\n').after('\n\n');
                            novel.summary = $('div')
                                .text()
                                .split('\n')
                                .map(function (line) { return line.trim(); })
                                .filter(function (line) { return line.length > 0; })
                                .join('\n\n')
                                .trim();
                        }
                        if (initialManga.ratingStats) {
                            novel.rating = initialManga.ratingStats.average;
                        }
                        cv = initialManga.content_version || initialManga.cv;
                        chaptersUrl = "".concat(this.api, "titles/").concat(initialManga.id, "/chapters").concat(cv ? "?cv=".concat(cv) : '');
                        return [4 /*yield*/, (0, fetch_1.fetchApi)(chaptersUrl)];
                    case 3:
                        chaptersResponse = _f.sent();
                        return [4 /*yield*/, chaptersResponse.json()];
                    case 4:
                        chaptersJson = _f.sent();
                        if ((chaptersJson === null || chaptersJson === void 0 ? void 0 : chaptersJson.success) && ((_e = chaptersJson === null || chaptersJson === void 0 ? void 0 : chaptersJson.data) === null || _e === void 0 ? void 0 : _e.chapters)) {
                            novel.chapters = chaptersJson.data.chapters
                                .map(function (chapter) { return ({
                                name: chapter.name,
                                path: (chapter.url.startsWith('/') ? chapter.url.slice(1) : chapter.url) +
                                    "?id=".concat(initialManga.id, "&chapterId=").concat(chapter.id),
                                releaseTime: chapter.updated_at,
                            }); })
                                .reverse();
                        }
                        else if (initialManga.chapters) {
                            novel.chapters = initialManga.chapters
                                .map(function (chapter) { return ({
                                name: chapter.name,
                                path: chapter.url.startsWith('/')
                                    ? chapter.url.slice(1)
                                    : chapter.url,
                                releaseTime: chapter.updatedAt,
                            }); })
                                .reverse();
                        }
                        return [2 /*return*/, novel];
                }
            });
        });
    };
    NovelBuddy.prototype.parseChapter = function (chapterPath) {
        return __awaiter(this, void 0, void 0, function () {
            var novelIdMatch, chapterIdMatch, content, novelId, chapterId, apiUrl, response, json, result, body, scriptMatch, data, initialChapter;
            var _a, _b;
            return __generator(this, function (_c) {
                switch (_c.label) {
                    case 0:
                        novelIdMatch = chapterPath.match(/[?&]id=([^&]+)/);
                        chapterIdMatch = chapterPath.match(/[?&]chapterId=([^&]+)/);
                        content = '';
                        if (!(novelIdMatch && chapterIdMatch)) return [3 /*break*/, 3];
                        novelId = novelIdMatch[1];
                        chapterId = chapterIdMatch[1];
                        apiUrl = "".concat(this.api, "titles/").concat(novelId, "/chapters/").concat(chapterId);
                        return [4 /*yield*/, (0, fetch_1.fetchApi)(apiUrl)];
                    case 1:
                        response = _c.sent();
                        return [4 /*yield*/, response.json()];
                    case 2:
                        json = _c.sent();
                        content = ((_b = (_a = json === null || json === void 0 ? void 0 : json.data) === null || _a === void 0 ? void 0 : _a.chapter) === null || _b === void 0 ? void 0 : _b.content) || '';
                        _c.label = 3;
                    case 3:
                        if (!!content) return [3 /*break*/, 6];
                        return [4 /*yield*/, (0, fetch_1.fetchApi)(this.site + chapterPath)];
                    case 4:
                        result = _c.sent();
                        return [4 /*yield*/, result.text()];
                    case 5:
                        body = _c.sent();
                        scriptMatch = body.match(/<script id="__NEXT_DATA__" type="application\/json">(.*?)<\/script>/);
                        if (!scriptMatch)
                            throw new Error('Could not find __NEXT_DATA__');
                        data = JSON.parse(scriptMatch[1]);
                        initialChapter = data.props.pageProps.initialChapter;
                        if (!initialChapter)
                            throw new Error('Could not find chapter content');
                        content = initialChapter.content;
                        _c.label = 6;
                    case 6:
                        if (content) {
                            content = content.replace(/Find authorized novels in Webnovel.*?faster updates, better experience.*?Please click www\.webnovel\.com for visiting\./gi, '');
                            content = content.replace(fwnRegex, '');
                        }
                        return [2 /*return*/, content];
                }
            });
        });
    };
    NovelBuddy.prototype.searchNovels = function (searchTerm, page) {
        return __awaiter(this, void 0, void 0, function () {
            var params, url, result, body;
            return __generator(this, function (_a) {
                switch (_a.label) {
                    case 0:
                        params = new URLSearchParams({
                            'q': searchTerm,
                            'limit': '24',
                            'page': page.toString(),
                        });
                        url = this.api + 'titles/search?' + params.toString();
                        return [4 /*yield*/, (0, fetch_1.fetchApi)(url)];
                    case 1:
                        result = _a.sent();
                        return [4 /*yield*/, result.json()];
                    case 2:
                        body = _a.sent();
                        return [2 /*return*/, this.parseNovels(body)];
                }
            });
        });
    };
    return NovelBuddy;
}());
exports.default = new NovelBuddy();
