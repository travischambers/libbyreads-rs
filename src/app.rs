use futures::{stream::FuturesUnordered, StreamExt};
use std::{future::Future, pin::Pin};

#[cfg(feature = "ssr")]
use tracing::info;

#[cfg(feature = "ssr")]
use tokio;

use std::time::Instant;

use crate::error_template::{AppError, ErrorTemplate};
use leptos::*;
use leptos_meta::*;
use leptos_router::*;

use std::sync::{Arc, Mutex};

use reqwest::Client;
use scraper::{Html, Selector};
use serde_json::Value;
use urlencoding::encode;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]

pub enum BookAvailability {
    Available,
    Holdable,
    NotOwned,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]

pub struct GoodreadsBook {
    cover: String,
    title: String,
    author: String,
    // date_added: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LibbyLibraryBook {
    cover: String,
    title: String,
    author: String,
    is_available: bool,
    is_holdable: bool,
    // we don't track is_owned directly, because we can infer it from is_available and is_holdable
    libby_search_url: String,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LibbyBook {
    cover: String,
    title: String,
    author: String,
    is_available: bool,
    is_holdable: bool,
    // we don't track is_owned directly, because we can infer it from is_available and is_holdable
    libby_search_url: String,
    library_books: Vec<LibbyLibraryBook>,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct SearchLibrary {
    system_name: String,    // Hawaii State Public Library System
    website_id: String,     // 50
    fulfillment_id: String, // hawaii
    name: String,           // Hawaii Kai Library
    branch_count: i32,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct Library {
    search_library: SearchLibrary,

    system_id: String,          // hawaii
    libby_base_url: String,     // https://libbyapp.com/library/hawaii
    overdrive_base_url: String, // https://thunder.api.overdrive.com/v2/libraries/hawaii
}

#[derive(Params, PartialEq)]
struct PageParams {
    user_id: String,
    libraries: String,
}

#[server(GetGoodreadsBooks, "/goodreads-books")]
pub async fn get_goodreads_books(user_id: String) -> Result<Vec<GoodreadsBook>, ServerFnError> {
    let start = Instant::now();

    let books = Arc::new(Mutex::new(Vec::new()));
    // URL of the user's to-read shelf
    // print=true here gives us a simpler webpage to parse
    // order=d sorts by descending
    // sort=date_added sorts by the order the books were added
    // TODO: get per_page to work. right now i always get 20
    // per_page=500 gives us 500 books at once. we could do more, but probably not necessary
    // TODO: make the shelf configurable via leptos multiselect dropdown
    let url = format!(
        "https://goodreads.com/review/list/{}?print=true&shelf=to-read",
        user_id
    );
    info!(user_id = user_id, url = url, "Fetching initial page.");
    // Parse the HTML document
    // the Html struct is not Sync, so we can't share it between threads
    // instead, we parse the document in a blocking tokio task
    let last_page = {
        let client = Client::new();
        let response = client.get(&url).send().await?.text().await?;
        let original_html = Html::parse_document(&response);
        info!(user_id = user_id, "Parsed html successfully.");
        // check for the `id=privateProfile` div, which indicates we won't be able to see any books
        let private_profile_selector = Selector::parse("#privateProfile").unwrap();
        if original_html
            .select(&private_profile_selector)
            .next()
            .is_some()
        {
            return Err(ServerFnError::ServerError("Private profile".to_string()));
        }
        // get the total number of pages
        let pagination_selector = Selector::parse("#reviewPagination a").unwrap();

        // Find the highest number in the pagination links
        let last_page = original_html
            .select(&pagination_selector)
            .filter_map(|element| element.text().collect::<String>().parse::<u32>().ok())
            .max()
            .unwrap_or(1); // If there are no pagination links, there is only one page

        // in rust, the last expression without a semicolon is implicitly returned
        last_page
    };

    let initial_page_duration = start.elapsed();
    info!(
        user_id = user_id,
        total_pages = last_page,
        duration_s = initial_page_duration.as_secs_f32(),
        "Parsed number of pages from initial page."
    );
    // Create async tasks for each page
    let mut tasks = vec![];
    for page_number in 1..=last_page {
        let books = Arc::clone(&books); // Clone the Arc for each task
        let client = Client::new();
        let page_url = format!("{}&page={}", url, page_number);
        info!(
            user_id = user_id,
            url = page_url,
            "Fetching Goodreads books."
        );

        // Spawn a new async task to fetch and parse the page
        let task = tokio::task::spawn(async move {
            if let Ok(response) = client.get(&page_url).send().await {
                if let Ok(text) = response.text().await {
                    let document = Html::parse_document(&text);

                    // i just looked at the HTML directly to determine these selectors
                    let book_rows_selector = Selector::parse("tr.bookalike.review").unwrap();
                    let cover_selector = Selector::parse("td.field.cover img").unwrap();
                    let title_selector = Selector::parse("td.field.title a").unwrap();
                    let author_selector = Selector::parse("td.field.author a").unwrap();
                    // let date_added_selector = Selector::parse("td.field.date_added span").unwrap();

                    // Loop through each book row
                    for book_row in document.select(&book_rows_selector) {
                        // Get cover image
                        let cover_element = book_row.select(&cover_selector).next().unwrap();
                        let cover = cover_element.value().attr("src").unwrap().to_string();

                        // Get title
                        let title_element = book_row.select(&title_selector).next().unwrap();
                        // Remove the span with the class darkGreyText, which Goodreads sometimes adds
                        // e.g. A Darker Shade of Magic <span class="darkGreyText">(Shades of Magic, #1)</span>
                        // should become A Darker Shade of Magic (Shades of Magic, #1)
                        // let title = title_element
                        //     .text()
                        //     .collect::<Vec<_>>()
                        //     .join("")
                        //     .trim()
                        //     .to_string();

                        let title = title_element
                            .children() // Get the child nodes of the <a> tag
                            .filter(|node| node.value().is_text()) // Filter to get only the text nodes (ignoring <span>)
                            .map(|node| node.value().as_text().unwrap().trim()) // Extract and trim the text
                            .collect::<Vec<_>>() // Collect the text parts
                            .join(" "); // Join them into a single string

                        // Get author
                        let author_element = book_row.select(&author_selector).next().unwrap();
                        let author = author_element.inner_html().trim().to_string();
                        // Get date added
                        // let date_added_element =
                        //     book_row.select(&date_added_selector).next().unwrap();
                        // let date_added = date_added_element.inner_html().trim().to_string();

                        // Create a book struct
                        let book = GoodreadsBook {
                            cover,
                            title,
                            author,
                            // date_added,
                        };

                        // Add the book to the shared vector
                        let mut books_guard = books.lock().unwrap();
                        books_guard.push(book);
                    }
                }
            }
        });
        tasks.push(task);
    }

    // Await all tasks
    for task in tasks {
        task.await?;
    }

    let books: std::sync::MutexGuard<'_, Vec<GoodreadsBook>> = books.lock().unwrap();
    let duration = start.elapsed();
    info!(
        user_id = user_id,
        initial_page_load_time=?initial_page_duration,
        all_pages_load_time=?duration,
        total_pages=last_page,
        total_books=books.len(),
        "Finished fetching all Goodreads pages."
    );
    Ok(books.clone())
}

#[server(GetLibbyAvailability, "/libby-availability")]
pub async fn get_libby_availability(
    book: GoodreadsBook,
    libraries: Vec<Library>,
) -> Result<LibbyBook, ServerFnError> {
    // TODO: search all configured libraries concurrently for each book
    let client = Client::new();
    let mut libby_library_books = Vec::new();
    let query = format!("{} {}", book.title, book.author);
    let url_safe_query = encode(&query);

    for library in &libraries {
        let libby_search_url: String = format!(
            "{}/search/query-{}/page-1",
            library.libby_base_url, url_safe_query
        );
        // TODO: make these formats configurable via leptos multiselect dropdown
        // let format_str: String = "format=ebook-overdrive,ebook-media-do,ebook-overdrive-provisional,audiobook-overdrive,audiobook-overdrive-provisional,magazine-overdrive".to_string();
        let format_str: String =
            "format=audiobook-overdrive,audiobook-overdrive-provisional".to_string();
        let overdrive_url = format!(
            "{}/media?query={}&{}&perPage=24&page=1&truncateDescription=false&x-client-id=dewey",
            library.overdrive_base_url, url_safe_query, format_str,
        );
        info!(
            title = book.title,
            author = book.author,
            library = library.search_library.system_name,
            libby_search_url = libby_search_url,
            "Searching for book.",
        );

        // Fetch the json from overdrive, then check the items array until we find a title that matches the book title

        // Fetch the page content
        let response = client
            .get(overdrive_url.clone())
            .send()
            .await?
            .text()
            .await?;

        // Parse the JSON document
        let json: Value = serde_json::from_str(&response).unwrap();
        let items = json["items"].as_array().unwrap();
        let mut book_found_at_library = false;
        for item in items {
            let title_replaced = item["title"].as_str().unwrap().replace("\n", "");
            let title: &str = title_replaced.trim();
            let author: &str = item["firstCreatorSortName"].as_str().unwrap();
            let is_available: bool = item["isAvailable"].as_bool().unwrap();
            let is_holdable: bool = item["isHoldable"].as_bool().unwrap();
            let cover: &str = item["covers"]["cover150Wide"]["href"].as_str().unwrap();

            if book.title.to_lowercase().starts_with(&title.to_lowercase())
                && author.to_lowercase() == book.author.to_lowercase()
            {
                let libby_library_book = LibbyLibraryBook {
                    cover: cover.to_string(),
                    title: title.to_string(),
                    author: author.to_string(),
                    is_available: is_available,
                    is_holdable: is_holdable,
                    libby_search_url: libby_search_url.to_string(),
                };
                libby_library_books.push(libby_library_book);
                book_found_at_library = true;
                break;
            }
        }
        if !book_found_at_library {
            info!(
                goodreads_title = book.title,
                goodreads_author = book.author,
                library = library.search_library.system_name,
                "Did not find book in libby.",
            );
            libby_library_books.push(LibbyLibraryBook {
                cover: "".to_string(),
                title: book.title.to_string(),
                author: book.author.to_string(),
                is_available: false,
                is_holdable: false,
                libby_search_url: libby_search_url.to_string(),
            })
        }
    }
    // find a library where `is_available` is true
    // if not found, find a library where `is_holdable` is true
    let mut is_available = false;
    let mut is_holdable = false;
    // initialize to the libby_search_url of the first library
    let mut libby_search_url = &libby_library_books[0].libby_search_url;
    for libby_library_book in libby_library_books.iter() {
        if libby_library_book.is_available {
            is_available = true;
            libby_search_url = &libby_library_book.libby_search_url;
            break;
        }
        if is_holdable == false && libby_library_book.is_holdable {
            is_holdable = true;
            libby_search_url = &libby_library_book.libby_search_url;
        }
    }
    let libby_book = LibbyBook {
        cover: book.cover.to_string(),
        title: book.title.to_string(),
        author: book.author.to_string(),
        is_available: is_available,
        is_holdable: is_holdable,
        libby_search_url: libby_search_url.to_string(),
        library_books: libby_library_books.clone(),
    };
    Ok(libby_book)
}

#[server(GetLibraries, "/libraries")]
pub async fn get_libraries(input: String) -> Result<Vec<SearchLibrary>, ServerFnError> {
    let client = Client::new();
    let url = format!("https://libbyapp.com/api/locate/autocomplete/{}", input);
    let response = client.get(&url).send().await?.text().await?;
    let json: Value = serde_json::from_str(&response).unwrap();
    let count = json["count"].as_i64().unwrap();
    let total = json["total"].as_i64().unwrap();
    info!(
        search_input = input,
        count = count,
        total = total,
        "Searching for library."
    );
    let branches = &json["branches"];
    let mut libraries = Vec::<SearchLibrary>::new();
    for branch in branches.as_array().unwrap_or(&vec![]) {
        // find the library system for this branch
        let system_name = branch["systems"][0]["name"].as_str().unwrap();
        // then check if this system is already in the libraries list
        if let Some(library) = libraries
            .iter_mut()
            .find(|lib| lib.system_name == system_name)
        {
            // if it is in the list, increment the branch count
            library.branch_count += 1;
        } else {
            // if not, add it to the list
            let fulfillment_id = branch["systems"][0]["fulfillmentId"].as_str().unwrap();

            let website_id = branch["systems"][0]["websiteId"]
                .as_i64()
                .unwrap()
                .to_string();

            let name = branch["name"].as_str().unwrap();
            libraries.push(SearchLibrary {
                system_name: system_name.to_string(),
                website_id: website_id.to_string(),
                fulfillment_id: fulfillment_id.to_string(),
                name: name.to_string(),
                branch_count: 1,
            });
        }
    }

    let found_system_names = libraries
        .iter()
        .map(|lib| lib.system_name.clone())
        .collect::<Vec<_>>()
        .join(", ");
    info!(
        num_systems=libraries.len(),
        found_system_names=?found_system_names,
        "Found library systems via libby autocomplete."
    );
    Ok(libraries)
}

#[server(GetLibraryFromWebsiteId, "/library-from-website-id")]
pub async fn get_library_from_website_id(website_id: String) -> Result<Library, ServerFnError> {
    let system_id_url = format!(
        "https://thunder.api.overdrive.com/v2/libraries/?websiteid={}",
        website_id
    );
    let client = Client::new();
    let library_json = client.get(&system_id_url).send().await?.text().await?;
    let library_value: Value = serde_json::from_str(&library_json)?;
    let system_id = library_value["items"][0]["id"].as_str().unwrap();
    let fulfillment_id = library_value["items"][0]["fulfillmentId"].as_str().unwrap();
    let name = library_value["items"][0]["name"].as_str().unwrap();
    let libby_base_url = format!("https://libbyapp.com/library/{}", system_id);
    let overdrive_base_url = format!(
        "https://thunder.api.overdrive.com/v2/libraries/{}",
        system_id
    );
    info!(
        website_id = website_id,
        method = "get_library_from_website_id",
        "Found library system!"
    );
    let search_lib = SearchLibrary {
        system_name: name.to_string(),
        website_id: website_id.to_string(),
        fulfillment_id: fulfillment_id.to_string(),
        name: name.to_string(),
        branch_count: 1,
    };
    Ok(Library {
        search_library: search_lib,
        system_id: system_id.to_string(),
        libby_base_url: libby_base_url,
        overdrive_base_url: overdrive_base_url,
    })
}

#[server(GetLibraryFromSystemId, "/library-from-system-id")]
pub async fn get_library_from_system_id(system_id: String) -> Result<Library, ServerFnError> {
    let system_id_url = format!(
        "https://thunder.api.overdrive.com/v2/libraries/{}",
        system_id
    );
    let client = Client::new();
    let library_json = client.get(&system_id_url).send().await?.text().await?;
    let library_value: Value = serde_json::from_str(&library_json)?;
    let name = library_value["name"].as_str().unwrap();
    let website_id = library_value["websiteId"].as_str().unwrap();
    let fulfillment_id = library_value["fulfillmentId"].as_str().unwrap();
    let libby_base_url = format!("https://libbyapp.com/library/{}", system_id);
    let overdrive_base_url = format!(
        "https://thunder.api.overdrive.com/v2/libraries/{}",
        system_id
    );
    let search_lib = SearchLibrary {
        system_name: name.to_string(),
        website_id: website_id.to_string(),
        fulfillment_id: fulfillment_id.to_string(),
        name: name.to_string(),
        branch_count: 1,
    };
    info!(
        search_lib = ?search_lib,
        method = "get_library_from_system_id",
        "Found library system."
    );
    Ok(Library {
        search_library: search_lib,
        system_id: system_id.to_string(),
        libby_base_url: libby_base_url,
        overdrive_base_url: overdrive_base_url,
    })
}

#[component]
pub fn App() -> impl IntoView {
    // Provides context that manages stylesheets, titles, meta tags, etc.
    provide_meta_context();

    view! {

        // water
        <Stylesheet href="https://cdn.jsdelivr.net/npm/water.css@2/out/water.css" />
        // holiday
        // <Stylesheet href="https://cdn.jsdelivr.net/npm/holiday.css@0.11.2" />

        // <meta name="viewport" content="width=device-width, initial-scale=1.0"/>

        <Stylesheet id="leptos" href="/pkg/libbyreads-rs.css"/>

        // sets the document title
        <Title text="LibbyReads"/>

        // content for this welcome page
        <Router fallback=|| {
            let mut outside_errors = Errors::default();
            outside_errors.insert_with_default_key(AppError::NotFound);
            view! {
                <ErrorTemplate outside_errors/>
            }
            .into_view()
        }>
            <main>
                <Routes>
                    <Route path="" view=HomePage/>
                </Routes>
            </main>
        </Router>
    }
}

#[component]
fn LibrarySearch(
    search_libraries: ReadSignal<Vec<SearchLibrary>>,
    set_search_libraries: WriteSignal<Vec<SearchLibrary>>,
    selected_library_website_ids: RwSignal<Vec<String>>,
) -> impl IntoView {
    let (search_input, set_search_input) = create_signal(String::new());

    let fetch_libraries = move |input: String| {
        spawn_local(async move {
            let trimmed_input = input.trim();
            if !trimmed_input.is_empty() {
                match get_libraries(trimmed_input.to_string()).await {
                    Ok(libs) => {
                        set_search_libraries.set(libs);
                    }
                    //TODO: what to do on error here?
                    Err(e) => {}
                }
            }
        });
    };

    let add_selected_library = move |library: SearchLibrary| {
        let mut curr_website_ids = selected_library_website_ids.get();
        if !curr_website_ids.contains(&library.website_id) {
            curr_website_ids.push(library.website_id.clone());
            selected_library_website_ids.set(curr_website_ids);
        }
    };

    let remove_selected_library = move |library: SearchLibrary| {
        let mut curr_website_ids = selected_library_website_ids.get();
        curr_website_ids.retain(|id| id != &library.website_id);
        selected_library_website_ids.set(curr_website_ids);
    };

    create_effect(move |_| {
        fetch_libraries(search_input.get());
    });

    view! {
        <h2> "Add Libraries" </h2>
        <input
            type="text"
            placeholder="Type a library name, your city, or zip code."
            on:input=move |e| set_search_input(event_target_value(&e))
            style="width: 95%;" // Adjust the width as needed
        />
        <table>
            <thead>
            <tr>
                <th style="width: 70%">"Library"</th>
                <th style="width: 30%">"Action"</th>
            </tr>
            </thead>
            <tbody>
            {move || search_libraries.get().iter().map(|library| {
                let library_clone = library.clone();
                let is_selected = selected_library_website_ids().contains(&library_clone.website_id);
                view! {
                <tr>
                    <td>{library.system_name.clone()}</td>
                    <td>
                    {if is_selected {
                        view! {
                        <button on:click=move |_| {
                            remove_selected_library(library_clone.clone());
                        }>"Remove"</button>
                        }
                    } else {
                        view! {
                        <button on:click=move |_| {
                            add_selected_library(library_clone.clone());
                        }>"Add"</button>
                        }
                    }}
                    </td>
                </tr>
                }
            }).collect::<Vec<_>>()}
            </tbody>
        </table>
    }
}

#[component]
fn DisplaySelectedLibraries(
    selected_libraries: RwSignal<Vec<Library>>,
    selected_library_website_ids: RwSignal<Vec<String>>,
) -> impl IntoView {
    let remove_selected_library = move |library: SearchLibrary| {
        let mut curr_website_ids = selected_library_website_ids.get();
        curr_website_ids.retain(|id| id != &library.website_id);
        selected_library_website_ids.set(curr_website_ids);
    };

    view! {
        <h2>"Selected Libraries"</h2>
        <table>
            <thead>
            <tr>
                <th style="width: 70%">"Library"</th>
                <th style="width: 30%">"Action"</th>
            </tr>
            </thead>
            <tbody>
            {move || selected_libraries.get().iter().map(|library| {
                let library_clone = library.clone();
                view! {
                <tr>
                    <td>{library.search_library.system_name.clone()}</td>
                    <td>
                        <button on:click=move |_| {remove_selected_library(library_clone.search_library.clone());}>
                            "Remove"
                        </button>
                    </td>
                </tr>
                }
            }).collect::<Vec<_>>()}
            </tbody>
        </table>
    }
}

#[component]
fn BookTable(
    books: ReadSignal<Vec<GoodreadsBook>>,
    availability: ReadSignal<Vec<LibbyBook>>,
    sort_by: ReadSignal<String>,
    sort_order: ReadSignal<String>,
    set_sort_by: WriteSignal<String>,
    set_sort_order: WriteSignal<String>,
) -> impl IntoView {
    view! {
        <table>
        <thead>
        <tr>
        <th on:click=move |_| {
        set_sort_by("cover".to_string());
        set_sort_order(if sort_by.get() == "cover" && sort_order.get() == "asc" { "desc".to_string() } else { "asc".to_string() });
        }>"Cover"</th>
        <th on:click=move |_| {
        set_sort_by("title".to_string());
        set_sort_order(if sort_by.get() == "title" && sort_order.get() == "asc" { "desc".to_string() } else { "asc".to_string() });
        }>"Title"</th>
        <th on:click=move |_| {
        set_sort_by("author".to_string());
        set_sort_order(if sort_by.get() == "author" && sort_order.get() == "asc" { "desc".to_string() } else { "asc".to_string() });
        }>"Author"</th>
        <th on:click=move |_| {
        set_sort_by("availability".to_string());
        set_sort_order(if sort_by.get() == "availability" && sort_order.get() == "desc" { "asc".to_string() } else { "desc".to_string() });
        }>"Libby Availability"</th>
        </tr>
        </thead>
        <tbody>
        {move || {
        let mut sorted_books = books.get().clone();
        sorted_books.sort_by(|a, b| {
            let order = match sort_by.get().as_str() {
            "cover" => a.cover.cmp(&b.cover),
            "title" => a.title.cmp(&b.title),
            "author" => a.author.cmp(&b.author),
            "availability" => {
                let availability_list = availability.get();
                let a_availability = availability_list.iter().find(|libby_book| libby_book.title == a.title && libby_book.author == a.author);
                let b_availability = availability_list.iter().find(|libby_book| libby_book.title == b.title && libby_book.author == b.author);
                match (a_availability, b_availability) {
                (Some(a_libby), Some(b_libby)) => {
                if a_libby.is_available && !b_libby.is_available {
                std::cmp::Ordering::Less
                } else if !a_libby.is_available && b_libby.is_available {
                std::cmp::Ordering::Greater
                } else if a_libby.is_holdable && !b_libby.is_holdable {
                std::cmp::Ordering::Less
                } else if !a_libby.is_holdable && b_libby.is_holdable {
                std::cmp::Ordering::Greater
                } else {
                std::cmp::Ordering::Equal
                }
                }
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
                }
            }
            _ => std::cmp::Ordering::Equal,
            };
            if sort_order.get() == "asc" {
            order
            } else {
            order.reverse()
            }
        });
        sorted_books.into_iter().map(|book| {
        let libby_book = availability.get().into_iter().find(|libby_book| libby_book.title == book.title && libby_book.author == book.author);
        view! {
        <tr>
            <td><img src={book.cover.clone()} alt="cover" /></td>
            <td>{book.title.clone()}</td>
            <td>{book.author.clone()}</td>
            <td>
            {match libby_book {
            Some(libby_book) if libby_book.is_available => view! {
                <a href={libby_book.libby_search_url.clone()} target="_blank">"AVAILABLE"</a>
            }.into_view(),
            Some(libby_book) if libby_book.is_holdable => view! {
                <a href={libby_book.libby_search_url.clone()} target="_blank">"HOLDABLE"</a>
            }.into_view(),
            Some(_) => view! {
                "NOT OWNED"
            }.into_view(),
            None => view! {
                "..."
            }.into_view(),
            }}
            </td>
        </tr>
        }
        }).collect::<Vec<_>>()
        }}
        </tbody>
    </table>
    }
}

#[component]
fn HomePage() -> impl IntoView {
    let (books, set_books) = create_signal(Vec::new());
    let is_private_profile = create_rw_signal(false);
    let (sort_by, set_sort_by) = create_signal(String::from("availability"));
    let (sort_order, set_sort_order) = create_signal(String::from("asc"));
    let (user_id, set_user_id) = create_signal(String::new());
    let (search_libraries, set_search_libraries) = create_signal(Vec::<SearchLibrary>::new());

    let selected_library_website_ids = create_rw_signal(Vec::<String>::new());
    let selected_libraries = create_rw_signal(Vec::<Library>::new());
    // selected_libraries is derived from selected_library_website_ids
    create_effect(move |_| {
        let selected_library_website_ids_clone = selected_library_website_ids.get().clone();

        // Remove the libraries that are no longer in `selected_library_website_ids`
        selected_libraries.update(|libs| {
            libs.retain(|lib| {
                selected_library_website_ids_clone
                    .iter()
                    .any(|website_id| &lib.search_library.website_id == website_id)
            });
        });

        // Filter out libraries that are already in the selected_libraries signal
        let new_libs_to_fetch = selected_library_website_ids_clone
            .iter()
            .filter(|website_id| {
                !selected_libraries
                    .get()
                    .iter()
                    .any(|lib| &lib.search_library.website_id == *website_id)
            })
            .cloned()
            .collect::<Vec<String>>();

        if new_libs_to_fetch.is_empty() {
            return; // No new libraries to fetch, exit early
        }

        let futures: Vec<_> = new_libs_to_fetch
            .into_iter()
            .map(|website_id| get_library_from_website_id(website_id))
            .collect();

        // Fetch libraries asynchronously and update the signal as they arrive
        spawn_local(async move {
            let mut libraries = Vec::new();
            for future in futures {
                if let Ok(lib) = future.await {
                    libraries.push(lib.clone());
                    // Now check before pushing to avoid duplicates
                    selected_libraries.update(|libs| {
                        if !libs.iter().any(|existing_lib| {
                            existing_lib.search_library.website_id == lib.search_library.website_id
                        }) {
                            libs.push(lib);
                        }
                    });
                }
            }
        });
    });
    let (libby_progress, set_libby_progress) = create_signal(0);
    let (available_count, set_available_count) = create_signal(0);
    let (holdable_count, set_holdable_count) = create_signal(0);
    let (not_owned_count, set_not_owned_count) = create_signal(0);
    let (availability, set_availability) = create_signal(Vec::new());

    let fetch_books = move || {
        let user_id = user_id.get();
        spawn_local(async move {
            match get_goodreads_books(user_id).await {
                Ok(fetched_books) => set_books.set(fetched_books),
                Err(e) => {
                    is_private_profile.update(|is_private| {
                        // TODO: this is a hacky way to check if the profile is private
                        // instead, figure out how to return a custom error from the server fn
                        // and check for that here
                        *is_private = e.to_string().contains("Private profile");
                    });
                }
            }
        });
    };

    let query = use_query::<PageParams>();
    let user_id_from_url = move || {
        query.with(|query| {
            query
                .as_ref()
                .map(|query| query.user_id.clone())
                .unwrap_or_default()
        })
    };
    let user_id_from_url_value = user_id_from_url();
    if !user_id_from_url_value.is_empty() {
        set_user_id(user_id_from_url_value);
        fetch_books();
    }

    // get a list of website ids from the url query param, if it exists
    let selected_library_website_ids_from_url = move || {
        query.with(|params: &Result<PageParams, ParamsError>| {
            if let Ok(params) = params.as_ref() {
                // libraries is a string like "50,34550,315"
                // we split it into a Vec of strings
                if !params.libraries.is_empty() {
                    params
                        .libraries
                        .split(",")
                        .map(|lib| lib.to_string())
                        .collect::<Vec<String>>()
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            }
        })
    };

    let selected_library_website_ids_from_url_value = selected_library_website_ids_from_url();
    if !selected_library_website_ids_from_url_value.is_empty() {
        selected_library_website_ids.set(selected_library_website_ids_from_url_value.clone());
    }
    logging::log!("User ID {:?}", user_id.get());
    logging::log!(
        "Selected libraries website IDs: {:?}",
        selected_library_website_ids.get()
    );

    let fetch_availability = move || {
        set_libby_progress.update(|progress| *progress = 0);
        set_available_count.update(|available| *available = 0);
        set_holdable_count.update(|holdable| *holdable = 0);
        set_not_owned_count.update(|not_owned| *not_owned = 0);
        set_availability.update(|availability| availability.clear());

        let books = books.get().clone();

        let fetch_concurrent = async move {
            let mut in_flight = FuturesUnordered::new();
            let mut book_iter = books.into_iter();
            let concurrency_limit = 5;

            // Start initial batch of requests (up to concurrency limit)
            for _ in 0..concurrency_limit {
                if let Some(book) = book_iter.next() {
                    let book_clone = book.clone();

                    // Wrap the async block in a Box to erase its type
                    let handle: Pin<Box<dyn Future<Output = ()> + 'static>> =
                        Box::pin(async move {
                            match get_libby_availability(book_clone, selected_libraries()).await {
                                Ok(fetched_availability) => {
                                    let availability_clone = fetched_availability.clone();
                                    set_availability.update(|availability| {
                                        availability.push(availability_clone);
                                    });
                                    if fetched_availability.is_available {
                                        set_available_count.update(|available| *available += 1);
                                    } else if fetched_availability.is_holdable {
                                        set_holdable_count.update(|holdable| *holdable += 1);
                                    } else {
                                        set_not_owned_count.update(|not_owned| *not_owned += 1);
                                    }
                                }
                                Err(_) => {
                                    // Handle error
                                }
                            }
                            set_libby_progress.update(|progress| *progress += 1);
                        });

                    in_flight.push(handle);
                }
            }

            // Process the queue dynamically, keeping <concurrency_limit> requests in flight at all times
            while let Some(_) = in_flight.next().await {
                // When a request finishes, start another if there are more books to process
                if let Some(book) = book_iter.next() {
                    let book_clone = book.clone();

                    // Wrap the async block in a Box to erase its type
                    let handle: Pin<Box<dyn Future<Output = ()> + 'static>> =
                        Box::pin(async move {
                            match get_libby_availability(book_clone, selected_libraries()).await {
                                Ok(fetched_availability) => {
                                    let availability_clone = fetched_availability.clone();
                                    set_availability.update(|availability| {
                                        availability.push(availability_clone);
                                    });
                                    if fetched_availability.is_available {
                                        set_available_count.update(|available| *available += 1);
                                    } else if fetched_availability.is_holdable {
                                        set_holdable_count.update(|holdable| *holdable += 1);
                                    } else {
                                        set_not_owned_count.update(|not_owned| *not_owned += 1);
                                    }
                                }
                                Err(_) => {
                                    // Handle error
                                }
                            }
                            set_libby_progress.update(|progress| *progress += 1);
                        });

                    in_flight.push(handle);
                }
            }
        };

        // Trigger the async function that controls concurrency
        spawn_local(fetch_concurrent);
    };

    view! {
            <h1>"LibbyReads"</h1>
            <p>"Fetch books from your Goodreads to-read shelf and check their availability at your libraries via Libby." </p>
            <input
                type="text"
                placeholder="Goodreads user ID"
                value=user_id.get()
                on:input=move |e| {
                    set_user_id(event_target_value(&e));
                    fetch_books();
                }
                title="Goodreads user ID"
            />
            {
                move || {
                let goodreads_url = format!("https://goodreads.com/review/list/{}?shelf=to-read", user_id.get());
                if user_id.get().is_empty() {
                    view! {
                    <div>
                        <p>"Enter your Goodreads user ID to get started. "
                            <a href="https://help.goodreads.com/s/article/Where-can-I-find-my-user-ID" target="_blank">
                            "Need help?"
                            </a>
                        </p>
                    </div>
                    }
                } else {
                    view! {
                    <div>
                        <p>
                            "Verify your Goodreads to-read shelf: "
                            <a href={goodreads_url.clone()} target="_blank">{goodreads_url}</a>
                        </p>
                        <hr />
                    </div>
                    }
                }
                }
            }
            <div>
                <div>
                    <LibrarySearch search_libraries=search_libraries set_search_libraries=set_search_libraries selected_library_website_ids=selected_library_website_ids />
                </div>
                <div>
                    <DisplaySelectedLibraries selected_libraries=selected_libraries selected_library_website_ids=selected_library_website_ids/>
                </div>
            </div>
            <button on:click=move |_| fetch_availability()>"Search"</button>
            // display summary of availability and progress bar
            <div>
                <p>{move || format!("Available: {}, Holdable: {}, Not Owned: {} -- {}/{}", available_count.get(), holdable_count.get(), not_owned_count.get(), libby_progress.get(), books.get().len())}</p>
                <progress style="width: 95%;" value=libby_progress max={move || books.get().len()}></progress>
            </div>
            <hr />
            // display books in a table if the user is not private
            {
                move || {
                if is_private_profile.get() {
                    view! {
                    <div>
                        <p style="color: #d9534f; font-weight: bold;">
                            "⚠ Your Goodreads profile is private. LibbyReads requires it to be public. "
                            "Edit your privacy settings via "
                            <a href="https://help.goodreads.com/s/article/How-do-I-edit-my-privacy-settings-1553870936907"
                            target="_blank" rel="noopener noreferrer" style="text-decoration: underline; color: #0275d8;">
                                "this guide on Goodreads"
                            </a>.
                        </p>
                    </div>
                    }
                } else {
                    view! {
                        <div>
                            <BookTable books=books availability=availability sort_by=sort_by sort_order=sort_order set_sort_by=set_sort_by set_sort_order=set_sort_order />
                        </div>
                    }
                }
            }
        }
    }
}
