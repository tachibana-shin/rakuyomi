// prettier-ignore
const mangayomiSources = [{
    "name": "Webtoons",
    "langs": ["en", "fr", "id", "th", "es", "zh", "de"],
    "baseUrl": "https://www.webtoons.com",
    "apiUrl": "",
    "iconUrl": "https://upload.wikimedia.org/wikipedia/commons/0/09/Naver_Line_Webtoon_logo.png",
    "typeSource": "single",
    "isManga": true,
    "isNsfw": false,
    "version": "0.0.45",
    "dateFormat": "",
    "dateFormatLocale": "",
    "pkgPath": "manga/src/all/webtoons.js"
}];

class DefaultExtension extends MProvider {
  headers = {
    "User-Agent":
      "Mozilla/5.0 (Linux; Android 13; Mobile) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/117.0.0.0 Mobile Safari/537.36",
  };

  mobileUrl = "https://m.webtoons.com";

  getFormattedUrl(preferenceKey) {
    const preference = new SharedPreferences();
    let url = preference.get(preferenceKey) || this.source.baseUrl;

    return url.endsWith("/") ? url.slice(0, -1) : url;
  }

  getBaseUrl() {
    return this.getFormattedUrl("domain_url");
  }

  getMobileUrl() {
    return this.getFormattedUrl("mobile_url");
  }

  mangaFromElement(doc) {
    const list = [];
    for (const el of doc.select(
      `div.webtoon_list_wrap li a, ul.webtoon_list li a`,
    )) {
      const imageUrl = el.selectFirst("img").getSrc;
      const name = el.selectFirst("strong.title").text;
      const link = el.getHref;
      list.push({ name, imageUrl, link });
    }

    return list;
  }

  async getPopular(page) {
    const res = await new Client().get(
      `${this.getBaseUrl()}/${this.langCode()}/originals`,
    );
    const doc = new Document(res.body);

    return {
      list: this.mangaFromElement(doc),
      hasNextPage: false,
    };
  }

  async getLatestUpdates(page) {
    const res = await new Client().get(
      `${this.getBaseUrl()}/${this.langCode()}/originals?sortOrder=UPDATE`,
    );
    const doc = new Document(res.body);

    return {
      list: this.mangaFromElement(doc),
      hasNextPage: false,
    };
  }

  async search(query, page, filters) {
    const keyword = query.trim().replace(/\s+/g, "+");
    let url = `${this.getBaseUrl()}/${this.langCode()}`;
    let hasNextPage = false;

    const getFilterValue = (type, defaultValue = "") => {
      const filter = filters.find((f) => f.type === type);
      return filter?.values?.[filter.state]?.value ?? defaultValue;
    };
    if (query) {
      url += `/search/${getFilterValue("searchType")}?keyword=${keyword}&page=${page}`;
    } else {
      const sortOrder = getFilterValue("sortOrder");
      const rankingType = getFilterValue("rankingType");
      const weekday = getFilterValue("weekday");
      const genreType = getFilterValue("genre");

      if (rankingType) {
        // const genreParam = genreType ? `&subTabGenreCode=${genreType}` : "";
        url += `/ranking/${rankingType}`;
      } else if (weekday) {
        url += `/originals/${weekday}?sortOrder=${sortOrder}`;
      } else if (genreType) {
        url += `/genres/${genreType}?sortOrder=${sortOrder}`;
      }
    }

    const res = await new Client().get(url);
    const doc = new Document(res.body);
    const list = this.mangaFromElement(doc);
    if (query) {
      hasNextPage = list.length !== 0;
    }

    return {
      list,
      hasNextPage,
    };
  }

  async getDetail(url) {
    let res = await new Client().get(url);
    let doc = new Document(res.body);

    const info = doc.selectFirst("div.cont_box");
    const name = info.selectFirst("h1.subj, h3.subj").text;
    const genre =
      Array.from(info.select("p.genre")).map((el) => el.text) != ""
        ? Array.from(info.select("p.genre")).map((el) => el.text)
        : [info.selectFirst("div.info h2").text];
    const author =
      info
        .selectFirst("div.author_area")
        .text.replace(/\s+/g, " ")
        .replace(/author info/g, "")
        .trim() ?? info.selectFirst("a.author").text;

    const dayInfoText = info?.selectFirst("p.day_info")?.text || "";
    const status =
      dayInfoText.includes("UP") ||
      dayInfoText.includes("EVERY") ||
      dayInfoText.includes("NOUVEAU")
        ? 0
        : dayInfoText.includes("END") ||
            dayInfoText.includes("TERMINÉ") ||
            dayInfoText.includes("COMPLETED")
          ? 1
          : -1; // UNKNOWN

    const description = info
      .selectFirst("p.summary")
      .text.replace(/\s+/g, " ")
      .trim();

    // chapters
    const chapters = [];
    res = await new Client().get(
      url.replace(this.getBaseUrl(), this.getMobileUrl()),
      this.headers,
    );
    doc = new Document(res.body);
    for (const el of doc.select("ul#_episodeList li[id*=episode] a")) {
      const url = el.getHref.replace(this.getMobileUrl(), this.getBaseUrl());
      let name = el.selectFirst(".sub_title > span.ellipsis")?.text;
      const chapterElement = el.selectFirst("div.row > div.num");
      if (chapterElement) {
        const chapterText = chapterElement.text;
        const hashIndex = chapterText.indexOf("#");
        if (hashIndex > -1) {
          name += " Ch. " + chapterText.substring(hashIndex + 1);
        }
      }
      const dateUpload = new Date(
        this.formatDateString(
          el.selectFirst(".sub_info .date")?.text,
          this.source.lang,
        ),
      )
        .getTime()
        .toString();

      chapters.push({
        name,
        url,
        dateUpload,
      });
    }

    return {
      name,
      link: url,
      genre,
      description,
      author,
      status,
      episodes: chapters,
    };
  }

  langCode() {
    return {
      en: "en",
      fr: "fr",
      id: "id",
      th: "th",
      es: "es",
      zh: "zh-hant",
      de: "de",
    }[this.source.lang];
  }

  formatDateString(dateStr, lang) {
    // Month translations for supported languages
    const monthTranslations = {
      en: [
        "Jan",
        "Feb",
        "Mar",
        "Apr",
        "May",
        "Jun",
        "Jul",
        "Aug",
        "Sep",
        "Oct",
        "Nov",
        "Dec",
      ],
      fr: [
        "janv.",
        "févr.",
        "mars",
        "avr.",
        "mai",
        "juin",
        "juil.",
        "août",
        "sept.",
        "oct.",
        "nov.",
        "déc.",
      ],
      id: [
        "Jan",
        "Feb",
        "Mar",
        "Apr",
        "Mei",
        "Jun",
        "Jul",
        "Agt",
        "Sep",
        "Okt",
        "Nov",
        "Des",
      ],
      th: [
        "ม.ค.",
        "ก.พ.",
        "มี.ค.",
        "เม.ย.",
        "พ.ค.",
        "มิ.ย.",
        "ก.ค.",
        "ส.ค.",
        "ก.ย.",
        "ต.ค.",
        "พ.ย.",
        "ธ.ค.",
      ],
      es: [
        "ene.",
        "feb.",
        "mar.",
        "abr.",
        "may.",
        "jun.",
        "jul.",
        "ago.",
        "sep.",
        "oct.",
        "nov.",
        "dic.",
      ],
      zh: [], // No need for month names; uses yyyy年MM月dd日 format
      de: [], // No need for month names; uses dd.MM.yyyy format
    };
    const months = monthTranslations[lang];
    let parts;
    let month;
    let day;
    let year;
    // Handle formats based on the language
    switch (lang) {
      case "zh":
        // Expected format: yyyy年MM月dd日
        const match = dateStr.match(/(\d{4})年(\d{1,2})月(\d{1,2})日/);
        year = match[1];
        month = match[2];
        day = match[3];
      case "de":
        // Expected format: dd.MM.yyyy
        parts = dateStr.split(".");
        if (parts.length === 3) {
          month = parts[1];
          day = parts[0];
          year = parts[2];
        }
      case "es":
      case "fr":
      case "id":
      case "th":
        // Expected format: dd MMM yyyy
        parts = dateStr.split(" ");
        if (parts.length === 3) {
          month = months.indexOf(parts[1]) + 1;
          day = parts[0];
          year = parts[2];
        }
        break;
      default:
        parts = dateStr.split(" ");
        if (parts.length === 3) {
          month = months.indexOf(parts[0]) + 1;
          day = parts[1].replace(",", "");
          year = parts[2];
        }
    }
    if (!month || !year || !day) {
      return Date.now();
    }
    return `${year}-${month.toString().padStart(2, "0")}-${day.toString().padStart(2, "0")}`;
  }

  async getPageList(url) {
    const res = await new Client().get(url);
    const doc = new Document(res.body);
    const urls = [];
    const imageElement = doc.selectFirst("div#_imageList");
    const img_urls = imageElement.select("img");
    for (let i = 0; i < img_urls.length; i++) {
      urls.push(img_urls[i].attr("data-url"));
    }
    return urls;
  }

  getFilterList() {
    return [
      {
        type: "header",
        name: "Filter Priority: Search > Ranking > Day > Genre | Sort applies to Day/Genre",
        type_name: "HeaderFilter",
      },
      {
        type: "separator",
        type_name: "SeparatorFilter",
      },

      {
        type: "searchType",
        name: "Search Type",
        type_name: "SelectFilter",
        values: [
          {
            type_name: "SelectOption",
            name: "Originals",
            value: "originals",
          },
          {
            type_name: "SelectOption",
            name: "Canvas",
            value: "canvas",
          },
        ],
        state: 0,
      },
      {
        type: "separator",
        type_name: "SeparatorFilter",
      },

      {
        type: "rankingType",
        name: "Ranking Category",
        type_name: "SelectFilter",
        values: [
          {
            type_name: "SelectOption",
            name: "Not Selected",
            value: "",
          },
          {
            type_name: "SelectOption",
            name: "Trending",
            value: "trending",
          },
          {
            type_name: "SelectOption",
            name: "Popular",
            value: "popular",
          },
          {
            type_name: "SelectOption",
            name: "Originals",
            value: "originals",
          },
          {
            type_name: "SelectOption",
            name: "Canvas",
            value: "canvas",
          },
        ],
      },
      {
        type: "separator",
        type_name: "SeparatorFilter",
      },

      {
        type: "sortOrder",
        name: "Sort By (For Schedule & Genres)",
        type_name: "SelectFilter",
        values: [
          { type_name: "SelectOption", name: "Popular (MANA)", value: "MANA" },
          { type_name: "SelectOption", name: "Likes", value: "LIKEIT" },
          { type_name: "SelectOption", name: "Newest", value: "UPDATE" },
        ],
        state: 0,
        appliesTo: ["weekday", "genre"],
      },

      {
        type: "weekday",
        name: "Update Schedule",
        type_name: "SelectFilter",
        values: [
          {
            type_name: "SelectOption",
            name: "Day",
            value: "",
            data: "",
          },
          {
            type_name: "SelectOption",
            name: "Monday",
            value: "monday",
            data: "MONDAY",
          },
          {
            type_name: "SelectOption",
            name: "Tuesday",
            value: "tuesday",
            data: "TUESDAY",
          },
          {
            type_name: "SelectOption",
            name: "Wednesday",
            value: "wednesday",
            data: "WEDNESDAY",
          },
          {
            type_name: "SelectOption",
            name: "Thursday",
            value: "thursday",
            data: "THURSDAY",
          },
          {
            type_name: "SelectOption",
            name: "Friday",
            value: "friday",
            data: "FRIDAY",
          },
          {
            type_name: "SelectOption",
            name: "Saturday",
            value: "saturday",
            data: "SATURDAY",
          },
          {
            type_name: "SelectOption",
            name: "Sunday",
            value: "sunday",
            data: "SUNDAY",
          },
          {
            type_name: "SelectOption",
            name: "Completed",
            value: "complete",
            data: "COMPLETE",
          },
        ],
      },

      {
        type: "genre",
        name: "Genre",
        type_name: "SelectFilter",
        values: [
          {
            type_name: "SelectOption",
            name: "All Genres",
            value: "",
            data: "",
          },
          {
            type_name: "SelectOption",
            name: "Drama",
            value: "drama",
            data: "DRAMA",
          },
          {
            type_name: "SelectOption",
            name: "Fantasy",
            value: "fantasy",
            data: "FANTASY",
          },
          {
            type_name: "SelectOption",
            name: "Comedy",
            value: "comedy",
            data: "COMEDY",
          },
          {
            type_name: "SelectOption",
            name: "Action",
            value: "action",
            data: "ACTION",
          },
          {
            type_name: "SelectOption",
            name: "Slice of Life",
            value: "slice_of_life",
            data: "SLICE_OF_LIFE",
          },
          {
            type_name: "SelectOption",
            name: "Romance",
            value: "romance",
            data: "ROMANCE",
          },
          {
            type_name: "SelectOption",
            name: "Superhero",
            value: "super_hero",
            data: "SUPER_HERO",
          },
          {
            type_name: "SelectOption",
            name: "Sci-Fi",
            value: "sf",
            data: "SF",
          },
          {
            type_name: "SelectOption",
            name: "Thriller",
            value: "thriller",
            data: "THRILLER",
          },
          {
            type_name: "SelectOption",
            name: "Supernatural",
            value: "supernatural",
            data: "SUPERNATURAL",
          },
          {
            type_name: "SelectOption",
            name: "Mystery",
            value: "mystery",
            data: "MYSTERY",
          },
          {
            type_name: "SelectOption",
            name: "Sports",
            value: "sports",
            data: "SPORTS",
          },
          {
            type_name: "SelectOption",
            name: "Historical",
            value: "historical",
            data: "HISTORICAL",
          },
          {
            type_name: "SelectOption",
            name: "Heartwarming",
            value: "heartwarming",
            data: "HEARTWARMING",
          },
          {
            type_name: "SelectOption",
            name: "Horror",
            value: "horror",
            data: "HORROR",
          },
          {
            type_name: "SelectOption",
            name: "Graphic Novel",
            value: "graphic_novel",
            data: "GRAPHIC_NOVEL",
          },
          {
            type_name: "SelectOption",
            name: "Informative",
            value: "tiptoon",
            data: "TIPTOON",
          },
        ],
      },
    ];
  }

  //  Preferences
  getSourcePreferences() {
    return [
      {
        key: "domain_url",
        editTextPreference: {
          title: "Override BaseUrl",
          summary: "",
          value: this.source.baseUrl,
          dialogTitle: "URL",
          dialogMessage: "",
        },
      },
      {
        key: "mobile_url",
        editTextPreference: {
          title: "Override mobileUrl",
          summary: "",
          value: this.mobileUrl,
          dialogTitle: "URL",
          dialogMessage: "",
        },
      },
    ];
  }
}
