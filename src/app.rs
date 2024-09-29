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
    availability: Option<BookAvailability>,
    libby_search_url: Option<String>,
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
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Library {
    system_name: String,
    fulfillment_id: String,
    name: String,
    street: String,
    city: String,
    region: String,
    zip: String,
    branch_count: i32,

    libby_base_url: String,
    overdrive_base_url: String,
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
    let url = format!("https://goodreads.com/review/list/{}?print=true&shelf=to-read", user_id);
    info!(url=url, "Fetching initial page.");
    // Parse the HTML document
    // the Html struct is not Sync, so we can't share it between threads
    // instead, we parse the document in a blocking tokio task
    let client = Client::new();
    let response = client.get(&url).send().await?.text().await?;
    let last_page = tokio::task::spawn_blocking(move || {
        let html = Html::parse_document(&response);
        info!("Parsed html successfully.");
        // get the total number of pages
        let pagination_selector = Selector::parse("#reviewPagination a").unwrap();

        // Find the highest number in the pagination links
        let last_page = html
            .select(&pagination_selector)
            .filter_map(|element| element.text().collect::<String>().parse::<u32>().ok())
            .max()
            .unwrap_or(1); // If there are no pagination links, there is only one page
        info!(total_pages=last_page, "Parsed number of pages from initial page.");
        last_page
    }).await?;

    let initial_page_duration = start.elapsed();
    // Create async tasks for each page
    let mut tasks = vec![];
    for page_number in 1..=last_page {
        let books = Arc::clone(&books); // Clone the Arc for each task
        let client = Client::new();
        let page_url = format!("{}&page={}", url, page_number);
        info!(url=page_url, "Fetching page at url.");

        // Spawn a new async task to fetch and parse the page
        let task = tokio::task::spawn(async move {
            if let Ok(response) = client.get(&page_url).send().await {
                if let Ok(text) = response.text().await {
                    let document = Html::parse_document(&text);
                    // Define selectors - i just looked at the HTML directly to determine these
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
                        let title = title_element
                            .text()
                            .collect::<Vec<_>>()
                            .join("")
                            .trim()
                            .to_string();

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
                            availability: None,
                            libby_search_url: None,
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
    info!(initial_page_load_time=?initial_page_duration, all_pages_load_time=?duration, total_pages=last_page, total_books=books.len(), "Finished fetching all Goodreads pages.");
    Ok(books.clone())
}

#[server(GetLibbyAvailability, "/libby-availability")]
pub async fn get_libby_availability(book: GoodreadsBook, libs: Vec<Library>) -> Result<LibbyBook, ServerFnError> {
    // TODO: search all configured libraries concurrently for each book
    // libs.push(Library {
    //     name: String::from("hawaii"),
    //     libby_base_url: String::from("https://libbyapp.com/library/hawaii"),
    //     overdrive_base_url: String::from("https://thunder.api.overdrive.com/v2/libraries/hawaii"),
    // });
    // libs.push(Library {
    //     name: String::from("utah"),
    //     libby_base_url: String::from("https://libbyapp.com/library/beehive"),
    //     overdrive_base_url: String::from("https://thunder.api.overdrive.com/v2/libraries/beehive"),
    // });
    // libs.push(Library {
    //     name: String::from("livermore"),
    //     libby_base_url: String::from("https://libbyapp.com/library/livermore"),
    //     overdrive_base_url: String::from("https://thunder.api.overdrive.com/v2/libraries/livermore"),
    // });
    // libs.push(Library {
    //     name: String::from("edmonton"),
    //     libby_base_url: String::from("https://libbyapp.com/library/epl"),
    //     overdrive_base_url: String::from("https://thunder.api.overdrive.com/v2/libraries/epl"),
    // });
    // libs.push(Library {
    //     name: String::from("georgia"),
    //     libby_base_url: String::from("https://libbyapp.com/library/gadd"),
    //     overdrive_base_url: String::from("https://thunder.api.overdrive.com/v2/libraries/gadd"),
    // });
    let client = Client::new();
    let mut libby_library_books = Vec::new();
    let query = format!("{} {}", book.title, book.author);
    let url_safe_query = encode(&query);

    for library in &libs {
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
            library.overdrive_base_url, 
            url_safe_query,
            format_str,
        );
        info!("Searching for book: {} by {} at {}", book.title, book.author, library.name);

        // Fetch the json from overdrive, then check the items array until we find a title that matches the book title

        // Fetch the page content
        let response = client.get(overdrive_url.clone()).send().await?.text().await?;

        // Parse the JSON document
        let json: Value = serde_json::from_str(&response).unwrap();
        let items = json["items"].as_array().unwrap();
        let mut book_found_at_library = false;
        for item in items {
            let title: &str = item["title"].as_str().unwrap();
            let author: &str = item["firstCreatorSortName"].as_str().unwrap();
            let is_available: bool = item["isAvailable"].as_bool().unwrap();
            let is_holdable: bool = item["isHoldable"].as_bool().unwrap();
            let cover: &str = item["covers"]["cover150Wide"]["href"].as_str().unwrap();
            info!("Found book in overdrive: {} by {} at {}\n", title, author, library.name);

            if title.to_lowercase() == book.title.to_lowercase()
                && author.to_lowercase() == book.author.to_lowercase()
            {
                // book.availability = Some(BookAvailability::Available);
                // book.libby_search_url = Some(libby_search_url.to_string());
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
            info!("Did not find book {} in libby at {}.", book.title, library.name);
            info!("{}", libby_search_url);
            info!("{}\n", overdrive_url);
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
pub async fn get_libraries(input: String) -> Result<Vec<Library>, ServerFnError> {
    let client = Client::new();
    let url = format!("https://libbyapp.com/api/locate/autocomplete/{}", input);
    let response = client.get(&url).send().await?.text().await?;
    let json: Value = serde_json::from_str(&response).unwrap();
    let count = json["count"].as_i64().unwrap();
    let total = json["total"].as_i64().unwrap();
    info!(search_input=input, count=count, total=total, "Searching for library.");
    let branches = &json["branches"];
    let mut libraries = Vec::<Library>::new();
    for branch in branches.as_array().unwrap_or(&vec![]) {
        // find the library system for this branch
        let system_name = branch["systems"][0]["name"].as_str().unwrap();
        // then check if this system is already in the libraries list
        if let Some(library) = libraries.iter_mut().find(|lib| lib.system_name == system_name) {
            // if it is in the list, increment the branch count
            library.branch_count += 1;
        } else {
            // if not, add it to the list
            let fulfillment_id = branch["systems"][0]["fulfillmentId"].as_str().unwrap();

            let name = branch["name"].as_str().unwrap();
            let street = branch["address"].as_str().unwrap();
            let city = branch["city"].as_str().unwrap();
            let region = branch["region"].as_str().unwrap();
            let zip = branch["postalCode"].as_str().unwrap();
            let libby_base_url = format!("https://libbyapp.com/library/{}", system_name);
            let overdrive_base_url = format!("https://thunder.api.overdrive.com/v2/libraries/{}", system_name);
            libraries.push(Library {
                system_name: system_name.to_string(),
                fulfillment_id: fulfillment_id.to_string(),
                name: name.to_string(),
                street: street.to_string(),
                city: city.to_string(),
                region: region.to_string(),
                zip: zip.to_string(),
                branch_count: 1,
                libby_base_url: libby_base_url,
                overdrive_base_url: overdrive_base_url,
            });
        }
    }

    let found_system_names = libraries.iter().map(|lib| lib.system_name.clone()).collect::<Vec<_>>().join(", ");
    info!(num_systems=libraries.len(), found_system_names=?found_system_names, "Found library systems.");
    Ok(libraries)
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
        <Title text="Libbyreads"/>

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
fn LibrarySelect(libraries: ReadSignal<Vec<Library>>, set_libraries: WriteSignal<Vec<Library>>) -> impl IntoView {
    let (search_input, set_search_input) = create_signal(String::new());


    let fetch_libraries = move |input: String| {
        spawn_local(async move {
            let trimmed_input = input.trim();
            if !trimmed_input.is_empty() {
                match get_libraries(trimmed_input.to_string()).await {
                    Ok(libs) => set_libraries.set(libs),
                    Err(e) => info!("{}", e.to_string()),
                }
            }
        });
    };

    create_effect(move |_| {
        fetch_libraries(search_input.get());
    });

    view! {
        <input
            type="text"
            placeholder="Try a library name, your city, or zip code."
            on:input=move |e| set_search_input(event_target_value(&e))
            style="width: 300px;" // Adjust the width as needed
        />
        <ul>
            {move || libraries.get().iter().map(|library| {
            view! {
                <li>{library.system_name.clone()}</li>
            }
            }).collect::<Vec<_>>()}
        </ul>
    }
}

#[component]
fn HomePage() -> impl IntoView { 
    let (books, set_books) = create_signal(Vec::new());
    let (sort_by, set_sort_by) = create_signal(String::from("title"));
    let (sort_order, set_sort_order) = create_signal(String::from("asc"));
    let (user_id, set_user_id) = create_signal(String::new());
    let (libraries, set_libraries) = create_signal(Vec::new());

    let fetch_books = move |_| {
        let user_id = user_id.get();
        spawn_local(async move {
            match get_goodreads_books(user_id).await {
                Ok(fetched_books) => set_books.set(fetched_books),
                Err(e) => {info!("{}", e.to_string())}, 
            }
        });
    };

    let (availability, set_availability) = create_signal(Vec::new());
    let fetch_availability = move || {
        let books = books.get().clone();
        for book in books.iter() {
            let book_clone = book.clone();
            spawn_local(async move {
                match get_libby_availability(book_clone, libraries.get()).await {
                    Ok(fetched_availability) => {
                        set_availability.update(|availability| {
                            availability.push(fetched_availability);
                        });
                    }
                    Err(e) => { info!("{}", e.to_string()) },
                }
            });
        }
        // print a count of is_available, is_holdable, and not_owned
        let mut available_count = 0;
        let mut holdable_count = 0;
        let mut not_owned_count = 0;
        for libby_book in availability.get().iter() {
            if libby_book.is_available {
                available_count += 1;
            }
            else if libby_book.is_holdable {
                holdable_count += 1;
            }
            else {
                not_owned_count += 1;
            }
        }
        // print available books
        info!("\n\nAvailable books: {}", available_count);
        for libby_book in availability.get().iter() {
            if libby_book.is_available {
                info!("{:?}", libby_book);
            }
        }

        // print holdable books
        info!("\n\nHoldable books: {}", holdable_count);
        for libby_book in availability.get().iter() {
            if libby_book.is_holdable {
                info!("{:?}", libby_book);
            }
        }
        // print summary
        // TODO: display this summary in app
        info!(
            available=available_count,
            holdable=holdable_count,
            not_owned=not_owned_count,
            total=availability.get().len(),
            "Summary.",
        );
    };

    view! {
        <h1>"Libbyreads"</h1>
        <p>"This app fetches books from your Goodreads to-read shelf and checks their availability at your library via Libby." </p>
        <input
            type="text"
            placeholder="44369181"
            on:input=move |e| set_user_id(event_target_value(&e))
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
                    "Verify your Goodreads to-read shelf:"
                    <br />
                <a href={goodreads_url.clone()} target="_blank">{goodreads_url}</a>
                </p>
                </div>
                }
            }
            }
        }
        <div>
            <LibrarySelect libraries=libraries set_libraries=set_libraries/>
        </div>
        <button on:click=fetch_books>"Fetch Goodreads Books"</button>
        <button on:click=move |_| fetch_availability()>"Fetch Libby Availability"</button>
        <br />
        // display books in a table
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
                set_sort_order(if sort_by.get() == "availability" && sort_order.get() == "asc" { "desc".to_string() } else { "asc".to_string() });
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

