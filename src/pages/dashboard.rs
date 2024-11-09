use crate::components::dashboard::*;
use leptos::prelude::*;

#[component]
pub fn DashboardPage() -> impl IntoView {
    view! {
        <section class="container">
            <div>
                <div>
                    <Interface/>
                    <Resume/>
                </div>
                <TableGroupsUser/>
            </div>
            <Traffics/>
        </section>
    }
}
