import time
from typing import Literal

from pydantic import BaseModel

from . import queries
from .queries.locate_button import LocateButtonResponse
from .koreader_driver import KOReaderDriver


class SearchViewModeResponse(BaseModel):
    mode: Literal['base', 'cover', 'grid']


async def get_search_view_mode(driver: KOReaderDriver) -> str:
    response = await driver.query(
        "What is the current view mode of the search results? "
        "Reply with 'base' if items are shown as a plain text list with no images, "
        "'cover' if items show cover art on the left side next to text, "
        "or 'grid' if items are arranged in multiple columns each showing cover art.",
        SearchViewModeResponse,
    )
    return response.mode


async def open_search(driver: KOReaderDriver, query: str) -> None:
    menu_button = await queries.locate_button(driver, "menu")
    driver.click_element(menu_button)

    search_button = await queries.locate_button(driver, "Search")
    driver.click_element(search_button)
    time.sleep(1)

    driver.type(query)
    search_button = await queries.locate_button(driver, "Search")
    driver.click_element(search_button)

    await driver.wait_for_event('manga_search_results_shown')


async def test_search_view_modes(koreader_driver: KOReaderDriver):
    await koreader_driver.install_source('multi.batoto')
    await koreader_driver.open_library_view()

    # Open an initial search
    await open_search(koreader_driver, 'houseki no kuni')

    # Default should be base (list) view
    mode = await get_search_view_mode(koreader_driver)
    assert mode == 'base', f"Expected default view mode 'base', got '{mode}'"

    # Tap toggle → cover
    toggle = await koreader_driver.query(
        "Locate the view mode toggle icon button in the top left corner of the title bar",
        LocateButtonResponse,
    )
    koreader_driver.click_element(toggle)
    await koreader_driver.wait_for_event('search_view_mode_changed')

    mode = await get_search_view_mode(koreader_driver)
    assert mode == 'cover', f"Expected view mode 'cover', got '{mode}'"

    # Tap toggle → grid
    toggle = await koreader_driver.query(
        "Locate the view mode toggle icon button in the top left corner of the title bar",
        LocateButtonResponse,
    )
    koreader_driver.click_element(toggle)
    await koreader_driver.wait_for_event('search_view_mode_changed')

    mode = await get_search_view_mode(koreader_driver)
    assert mode == 'grid', f"Expected view mode 'grid', got '{mode}'"

    # Close search and reopen — mode should persist
    back_button = await queries.locate_button(koreader_driver, "Back")
    koreader_driver.click_element(back_button)

    await open_search(koreader_driver, 'houseki no kuni')

    mode = await get_search_view_mode(koreader_driver)
    assert mode == 'grid', f"Expected persisted view mode 'grid', got '{mode}'"
