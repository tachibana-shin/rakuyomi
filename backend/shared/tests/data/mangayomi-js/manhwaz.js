// prettier-ignore
const mangayomiSources = [{
    "name": "ManhwaZ",
    "lang": "en",
    "baseUrl": "https://manhwaz.com",
    "apiUrl": "",
    "iconUrl": "https://manhwaz.com/apple-touch-icon.png",
    "typeSource": "single",
    "itemType": 0,
    "version": "0.1.0",
    "pkgPath": "manga/src/en/manhwaz.js",
    "notes": ""
}];

class DefaultExtension extends MProvider {
  getHeaders(url) {
    return {
      Referer: this.source.baseUrl,
      "User-Agent":
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36",
    };
  }

  // Helper method to parse manga list from page
  mangaListFromPage(res, selector = ".page-item-detail") {
    const doc = new Document(res.body);
    const list = [];

    // Look for manga items using the specified selector
    const mangaElements = doc.select(selector);

    for (const element of mangaElements) {
      let linkElement, titleElement, imageElement;
      let name = "";
      let imageUrl = "";
      let link = "";

      if (selector === "#slide-top > .item") {
        // Popular manga from homepage
        linkElement = element.selectFirst(".info-item a");
        if (linkElement) {
          name = linkElement.text;
          link = linkElement.attr("href");
        }
        imageElement = element.selectFirst(".img-item img");
      } else {
        // Latest updates and search results
        linkElement = element.selectFirst(".item-summary a");
        if (linkElement) {
          name = linkElement.text;
          link = linkElement.attr("href");
        }
        imageElement = element.selectFirst(".item-thumb img");
      }

      if (imageElement) {
        imageUrl = this.getImageUrl(imageElement);
      }

      if (name && link) {
        list.push({ name, imageUrl, link });
      }
    }

    // Check for next page
    const hasNextPage = doc.selectFirst("ul.pager a[rel=next]") !== null;

    return { list: list, hasNextPage };
  }

  // Helper method to get image URL with fallbacks
  getImageUrl(imageElement) {
    if (imageElement.attr("data-src")) {
      return imageElement.attr("data-src");
    } else if (imageElement.attr("data-lazy-src")) {
      return imageElement.attr("data-lazy-src");
    } else if (imageElement.attr("srcset")) {
      return imageElement.attr("srcset").split(" ")[0];
    } else if (imageElement.attr("data-cfsrc")) {
      return imageElement.attr("data-cfsrc");
    } else {
      return imageElement.attr("src") || "";
    }
  }

  // Convert status text to status code
  toStatus(status) {
    const statusLower = status?.toLowerCase() || "";
    if (statusLower.includes("ongoing") || statusLower.includes("publishing")) {
      return 0;
    } else if (
      statusLower.includes("completed") ||
      statusLower.includes("complete")
    ) {
      return 1;
    } else if (statusLower.includes("hiatus")) {
      return 2;
    } else if (
      statusLower.includes("cancelled") ||
      statusLower.includes("dropped")
    ) {
      return 3;
    } else {
      return 5; // unknown
    }
  }

  // Parse relative date string to milliseconds
  parseRelativeDate(dateStr) {
    if (!dateStr) return String(new Date().valueOf());

    try {
      const lowerDateStr = dateStr.toLowerCase().trim();
      const now = new Date();

      // Extract number and unit
      const match = lowerDateStr.match(
        /(\d+)\s*(second|minute|hour|day|week|month|year)s?\s*ago/,
      );
      if (!match) {
        // Try to parse as regular date
        const date = new Date(dateStr);
        return String(date.valueOf());
      }

      const value = parseInt(match[1]);
      const unit = match[2];

      const calendar = new Date(now);

      switch (unit) {
        case "second":
          calendar.setSeconds(calendar.getSeconds() - value);
          break;
        case "minute":
          calendar.setMinutes(calendar.getMinutes() - value);
          break;
        case "hour":
          calendar.setHours(calendar.getHours() - value);
          break;
        case "day":
          calendar.setDate(calendar.getDate() - value);
          break;
        case "week":
          calendar.setDate(calendar.getDate() - value * 7);
          break;
        case "month":
          calendar.setMonth(calendar.getMonth() - value);
          break;
        case "year":
          calendar.setFullYear(calendar.getFullYear() - value);
          break;
        default:
          return String(now.valueOf());
      }

      return String(calendar.valueOf());
    } catch (e) {
      return String(new Date().valueOf());
    }
  }

  async getPopular(page) {
    const url = `${this.getBaseUrl()}/genre/manhwa?page=${page}&m_orderby=views`;
    const res = await new Client().get(url, this.getHeaders());
    return this.mangaListFromPage(res, ".page-item-detail");
  }

  get supportsLatest() {
    return true;
  }

  async getLatestUpdates(page) {
    const url = `${this.getBaseUrl()}/?page=${page}`;
    const res = await new Client().get(url, this.getHeaders());
    return this.mangaListFromPage(res, ".page-item-detail");
  }

  async search(query, page, filters) {
    if (query && query.trim()) {
      // Search with query
      const url = `${this.getBaseUrl()}/search?s=${encodeURIComponent(query)}&page=${page}`;
      const res = await new Client().get(url, this.getHeaders());
      return this.mangaListFromPage(res, ".page-item-detail");
    }

    // Filter-based search
    let url = this.getBaseUrl();
    let hasGenreFilter = false;

    // Process filters
    if (filters && filters.length > 0) {
      const genreFilter = filters.find(
        (f) => f.type === "select" && f.name === "genre",
      );
      const orderByFilter = filters.find(
        (f) => f.type === "select" && f.name === "orderby",
      );

      if (genreFilter && genreFilter.state > 0) {
        const selectedGenre = genreFilter.values[genreFilter.state];
        if (selectedGenre && selectedGenre.value) {
          url += "/" + selectedGenre.value;
          hasGenreFilter = true;
        }
      }

      // Add order by parameter for genre pages
      if (hasGenreFilter && orderByFilter && orderByFilter.state > 0) {
        const selectedOrder = orderByFilter.values[orderByFilter.state];
        if (selectedOrder && selectedOrder.value) {
          url += `?m_orderby=${selectedOrder.value}`;
          url += `&page=${page}`;
        } else {
          url += `?page=${page}`;
        }
      } else {
        url += `?page=${page}`;
      }
    } else {
      url += `?page=${page}`;
    }

    const res = await new Client().get(url, this.getHeaders());
    return this.mangaListFromPage(res, ".page-item-detail");
  }

  async getDetail(url) {
    // Ensure we have the full URL
    const fullUrl = url.startsWith("http") ? url : `${this.getBaseUrl()}${url}`;
    const res = await new Client().get(fullUrl, this.getHeaders());
    const doc = new Document(res.body);

    // Extract manga details based on Kotlin implementation
    const title = doc.selectFirst("div.post-title h1")?.text || "";

    const descElement = doc.selectFirst("div.summary__content");
    const description = descElement?.text?.trim() || "";

    const imageElement = doc.selectFirst("div.summary_image img");
    const imageUrl = imageElement ? this.getImageUrl(imageElement) : "";

    // Extract author
    const authorElement = doc.selectFirst(
      "div.post-content_item .summary-heading:contains(Author) + .summary-content",
    );
    const author = authorElement?.text?.trim() || "";

    // Extract status
    const statusElement = doc.selectFirst(
      "div.summary-heading:contains(status) + div.summary-content",
    );
    const statusText = statusElement?.text?.toLowerCase() || "";
    const status = this.toStatus(statusText);

    // Extract genres
    const genre = [];
    const genreElements = doc.select("div.genres-content a[rel=tag]");
    for (const genreEl of genreElements) {
      const genreText = genreEl.text?.trim();
      if (genreText) {
        genre.push(genreText);
      }
    }

    // Extract chapters
    const chapters = [];
    const chapterElements = doc.select("li.wp-manga-chapter");

    for (const chapterEl of chapterElements) {
      const chapterLink = chapterEl.selectFirst("a");
      if (!chapterLink) continue;

      const chapterUrl = chapterLink.attr("href");
      const chapterName = chapterLink.text?.trim() || "";

      // Try to get upload date
      const dateElement = chapterEl.selectFirst("span.chapter-release-date");
      const dateUpload = this.parseRelativeDate(dateElement?.text);

      if (chapterName && chapterUrl) {
        chapters.push({
          name: chapterName,
          url: chapterUrl,
          dateUpload,
        });
      }
    }

    return {
      title,
      description,
      imageUrl,
      status,
      author,
      genre,
      chapters,
    };
  }

  async getPageList(url) {
    // Ensure we have the full URL
    const fullUrl = url.startsWith("http") ? url : `${this.getBaseUrl()}${url}`;
    const res = await new Client().get(fullUrl, this.getHeaders());
    const doc = new Document(res.body);

    const pages = [];

    // Look for images in page break containers as per Kotlin implementation
    const imageElements = doc.select("div.page-break img");

    for (const img of imageElements) {
      const imageUrl = this.getImageUrl(img);

      if (imageUrl) {
        // Handle relative URLs
        let finalUrl = imageUrl;
        if (imageUrl.startsWith("//")) {
          finalUrl = "https:" + imageUrl;
        } else if (imageUrl.startsWith("/")) {
          finalUrl = this.getBaseUrl() + imageUrl;
        }

        pages.push(finalUrl);
      }
    }

    return pages;
  }

  getFilterList() {
    return [
      {
        type: "header",
        name: "Note: Filters only work when search query is empty",
      },
      {
        type: "separator",
      },
      {
        type: "select",
        name: "genre",
        label: "Genre",
        values: [
          { name: "All", value: "" },
          { name: "Completed", value: "completed" },
          { name: "Action", value: "genre/action" },
          { name: "Adult", value: "genre/adult" },
          { name: "Adventure", value: "genre/adventure" },
          { name: "Comedy", value: "genre/comedy" },
          { name: "Drama", value: "genre/drama" },
          { name: "Ecchi", value: "genre/ecchi" },
          { name: "Fantasy", value: "genre/fantasy" },
          { name: "Harem", value: "genre/harem" },
          { name: "Historical", value: "genre/historical" },
          { name: "Horror", value: "genre/horror" },
          { name: "Isekai", value: "genre/isekai" },
          { name: "Josei", value: "genre/josei" },
          { name: "Manhwa", value: "genre/manhwa" },
          { name: "Martial Arts", value: "genre/martial-arts" },
          { name: "Mature", value: "genre/mature" },
          { name: "Mecha", value: "genre/mecha" },
          { name: "Mystery", value: "genre/mystery" },
          { name: "Psychological", value: "genre/psychological" },
          { name: "Romance", value: "genre/romance" },
          { name: "School Life", value: "genre/school-life" },
          { name: "Sci-fi", value: "genre/sci-fi" },
          { name: "Seinen", value: "genre/seinen" },
          { name: "Shoujo", value: "genre/shoujo" },
          { name: "Shounen", value: "genre/shounen" },
          { name: "Slice of Life", value: "genre/slice-of-life" },
          { name: "Sports", value: "genre/sports" },
          { name: "Supernatural", value: "genre/supernatural" },
          { name: "Tragedy", value: "genre/tragedy" },
          { name: "Webtoons", value: "genre/webtoons" },
        ],
        state: 0,
      },
      {
        type: "select",
        name: "orderby",
        label: "Order By",
        values: [
          { name: "Latest", value: "latest" },
          { name: "Rating", value: "rating" },
          { name: "Most Views", value: "views" },
          { name: "New", value: "new" },
        ],
        state: 0,
      },
    ];
  }

  getBaseUrl() {
    const preference = new SharedPreferences();
    var base_url = preference.get("domain_url");
    if (base_url.length == 0) {
      return this.source.baseUrl;
    }
    if (base_url.endsWith("/")) {
      return base_url.slice(0, -1);
    }
    return base_url;
  }

  getSourcePreferences() {
    return [
      {
        key: "domain_url",
        editTextPreference: {
          title: "Edit URL",
          summary: "",
          value: this.source.baseUrl,
          dialogTitle: "URL",
          dialogMessage: "",
        },
      },
    ];
  }
}
